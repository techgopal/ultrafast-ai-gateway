//! Turning a streaming HTTP body into events, with no silent truncation.

use std::time::Duration;

use futures::{Stream, StreamExt};
use ultrafast_translate::provider::{ProviderKind, StreamDecoder};
use ultrafast_translate::types::StreamEvent;

use crate::error::{Error, ErrorKind};

/// Events from `body`. Ends after `Done`, or after one `Err`: a decoder
/// error, a broken connection, a silent stretch longer than `idle`, or a
/// close before the stream was complete.
pub(crate) fn events(
    kind: ProviderKind,
    resp: reqwest::Response,
    idle: Duration,
    key: Option<String>,
) -> impl Stream<Item = Result<StreamEvent, Error>> {
    let scrub = move |e: Error| e.scrubbed(key.as_deref());
    async_stream::stream! {
        let mut decoder = StreamDecoder::new(kind);
        let mut body = Box::pin(resp.bytes_stream());
        let mut done = false;
        loop {
            let next = match tokio::time::timeout(idle, body.next()).await {
                Ok(next) => next,
                Err(_) => {
                    yield Err(scrub(Error::new(ErrorKind::Timeout, "the stream went quiet")));
                    return;
                }
            };
            let chunk = match next {
                Some(Ok(chunk)) => chunk,
                Some(Err(e)) => {
                    yield Err(scrub(Error::from_reqwest(e)));
                    return;
                }
                None => break,
            };
            let fed = decoder.feed(&chunk);
            let events = match fed {
                Ok(events) => events,
                Err(e) => {
                    yield Err(scrub(Error::from_translate(e, None)));
                    return;
                }
            };
            for event in events {
                done |= matches!(event, StreamEvent::Done { .. });
                yield Ok(event);
            }
            if let Some(e) = decoder.take_error() {
                yield Err(scrub(Error::from_translate(e, None)));
                return;
            }
            if done {
                return;
            }
        }
        for event in decoder.finish() {
            done |= matches!(event, StreamEvent::Done { .. });
            yield Ok(event);
        }
        if !done {
            yield Err(scrub(Error::new(
                ErrorKind::Malformed,
                "the stream ended before it was complete",
            )));
        }
    }
}
