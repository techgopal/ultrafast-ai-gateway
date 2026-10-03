use std::fmt;
use std::time::Duration;

use futures::Stream;
use ultrafast_translate::embeddings;
use ultrafast_translate::embeddings::EmbeddingsResponse;
use ultrafast_translate::provider::{self, HttpRequest};
use ultrafast_translate::types::{ChatResponse, StreamEvent};

use crate::error::{Error, ErrorKind};
use crate::request::{tags_header, ChatRequest, EmbeddingsRequest};
use crate::stream;
use crate::target::Target;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// A `Retry-After` longer than this is capped.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
pub struct Client {
    target: Target,
    http: reqwest::Client,
    timeout: Duration,
}

/// Never prints the key.
impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("target", &self.target)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl Client {
    pub fn new(target: Target) -> Client {
        // Redirects are not followed: a provider key header would go along.
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Client {
            target,
            http,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// For `chat` and `embed`, the whole call; for `chat_stream`, the wait
    /// for the answer and the longest silence between two chunks.
    pub fn with_timeout(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    /// Uses your own HTTP client (proxy, TLS roots, pooling). It is yours to
    /// make safe: do not let it follow redirects with credentials.
    pub fn with_http(mut self, http: reqwest::Client) -> Client {
        self.http = http;
        self
    }

    pub async fn chat(&self, request: impl Into<ChatRequest>) -> Result<ChatResponse, Error> {
        self.chat_inner(request.into())
            .await
            .map_err(|e| self.scrub(e))
    }

    async fn chat_inner(&self, mut request: ChatRequest) -> Result<ChatResponse, Error> {
        request.inner.stream = false;
        let target = self.target.translate(&request.inner.model);
        let http = provider::build_request(&target, &request.inner)
            .map_err(|e| Error::from_translate(e, None))?;
        let tags = self.tags(&request.tags)?;
        let resp = self.send(http, tags, Some(self.timeout), false).await?;
        let (status, retry_after, body) = read(resp).await?;
        check_redirect(status)?;
        provider::parse_response(self.target.kind(), status, &body)
            .map_err(|e| Error::from_translate(e, retry_after))
    }

    /// Events in order, then at most one error. A stream that ends before
    /// its `Done` ends with an error, never silently.
    pub async fn chat_stream(
        &self,
        request: impl Into<ChatRequest>,
    ) -> Result<impl Stream<Item = Result<StreamEvent, Error>>, Error> {
        self.stream_inner(request.into())
            .await
            .map_err(|e| self.scrub(e))
    }

    async fn stream_inner(
        &self,
        mut request: ChatRequest,
    ) -> Result<impl Stream<Item = Result<StreamEvent, Error>>, Error> {
        request.inner.stream = true;
        let target = self.target.translate(&request.inner.model);
        let http = provider::build_request(&target, &request.inner)
            .map_err(|e| Error::from_translate(e, None))?;
        let tags = self.tags(&request.tags)?;
        let resp = match tokio::time::timeout(self.timeout, self.send(http, tags, None, true)).await
        {
            Ok(r) => r?,
            Err(_) => return Err(Error::new(ErrorKind::Timeout, "the request timed out")),
        };
        let status = resp.status().as_u16();
        if status >= 300 {
            let (status, retry_after, body) = read(resp).await?;
            check_redirect(status)?;
            return Err(
                match provider::parse_response(self.target.kind(), status, &body) {
                    Err(e) => Error::from_translate(e, retry_after),
                    Ok(_) => Error::new(ErrorKind::Malformed, "unexpected answer"),
                },
            );
        }
        let key = Some(self.target.key().to_string());
        Ok(stream::events(self.target.kind(), resp, self.timeout, key))
    }

    pub async fn embed(
        &self,
        request: impl Into<EmbeddingsRequest>,
    ) -> Result<EmbeddingsResponse, Error> {
        self.embed_inner(request.into())
            .await
            .map_err(|e| self.scrub(e))
    }

    async fn embed_inner(&self, request: EmbeddingsRequest) -> Result<EmbeddingsResponse, Error> {
        let target = self.target.translate(&request.inner.model);
        let http = embeddings::build_request(&target, &request.inner)
            .map_err(|e| Error::from_translate(e, None))?;
        let tags = self.tags(&request.tags)?;
        let resp = self.send(http, tags, Some(self.timeout), false).await?;
        let (status, retry_after, body) = read(resp).await?;
        check_redirect(status)?;
        embeddings::parse_response(self.target.kind(), status, &body, &request.inner.model)
            .map_err(|e| Error::from_translate(e, retry_after))
    }

    /// The tags header, for a gateway target only; providers never see tags.
    fn tags(
        &self,
        tags: &std::collections::BTreeMap<String, String>,
    ) -> Result<Option<String>, Error> {
        if !self.target.is_gateway() {
            return Ok(None);
        }
        tags_header(tags)
    }

    async fn send(
        &self,
        http: HttpRequest,
        tags: Option<String>,
        timeout: Option<Duration>,
        sse: bool,
    ) -> Result<reqwest::Response, Error> {
        let mut builder = self.http.post(&http.url).body(http.body);
        for (k, v) in &http.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        if sse {
            builder = builder.header("accept", "text/event-stream");
        }
        if let Some(tags) = tags {
            builder = builder.header("x-uf-tags", tags);
        }
        if let Some(t) = timeout {
            builder = builder.timeout(t);
        }
        builder.send().await.map_err(Error::from_reqwest)
    }

    fn scrub(&self, e: Error) -> Error {
        e.scrubbed(Some(self.target.key()))
    }
}

fn check_redirect(status: u16) -> Result<(), Error> {
    if (300..400).contains(&status) {
        return Err(Error::from_status(
            status,
            "the server answered with a redirect",
            None,
        ));
    }
    Ok(())
}

async fn read(resp: reqwest::Response) -> Result<(u16, Option<Duration>, bytes::Bytes), Error> {
    let status = resp.status().as_u16();
    let retry_after = retry_after_of(resp.headers());
    let body = resp.bytes().await.map_err(Error::from_reqwest)?;
    Ok((status, retry_after, body))
}

/// `Retry-After` as seconds; an HTTP date is not read.
fn retry_after_of(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Digits too many for a number are a very long wait: the cap.
    Some(
        value
            .parse::<u64>()
            .map(Duration::from_secs)
            .unwrap_or(RETRY_AFTER_CAP)
            .min(RETRY_AFTER_CAP),
    )
}
