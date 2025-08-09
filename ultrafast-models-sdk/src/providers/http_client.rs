use crate::error::ProviderError;
use reqwest::{header::HeaderMap, Client, Method, Response};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum AuthStrategy {
    Bearer { token: String },
    Header { name: String, value: String },
    None,
}

#[derive(Clone)]
pub struct HttpProviderClient {
    http: Client,
    base_url: String,
    default_headers: HeaderMap,
}

impl HttpProviderClient {
    pub fn new(
        timeout: Duration,
        base_url: Option<String>,
        default_base: &str,
        headers: &HashMap<String, String>,
        auth: AuthStrategy,
    ) -> Result<Self, ProviderError> {
        let http = Client::builder().timeout(timeout).build().map_err(|e| {
            ProviderError::Configuration {
                message: format!("Failed to create HTTP client: {e}"),
            }
        })?;

        let mut default_headers = HeaderMap::new();

        match auth {
            AuthStrategy::Bearer { token } => {
                default_headers.insert("Authorization", format!("Bearer {token}").parse().unwrap());
            }
            AuthStrategy::Header { name, value } => {
                if let (Ok(name), Ok(value)) =
                    (name.parse::<reqwest::header::HeaderName>(), value.parse())
                {
                    default_headers.insert(name, value);
                }
            }
            AuthStrategy::None => {}
        }

        for (k, v) in headers {
            if let (Ok(name), Ok(value)) = (k.parse::<reqwest::header::HeaderName>(), v.parse()) {
                default_headers.insert(name, value);
            }
        }

        let base_url = base_url.unwrap_or_else(|| default_base.to_string());

        Ok(Self {
            http,
            base_url,
            default_headers,
        })
    }

    fn build_url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/{}", self.base_url.trim_end_matches('/'), path)
        }
    }

    fn build_headers(&self) -> HeaderMap {
        self.default_headers.clone()
    }

    pub async fn post_json<TReq: Serialize, TResp: DeserializeOwned>(
        &self,
        path: &str,
        body: &TReq,
    ) -> Result<TResp, ProviderError> {
        let url = self.build_url(path);
        let resp = self
            .http
            .request(Method::POST, url)
            .headers(self.build_headers())
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(map_error_response(resp).await);
        }
        Ok(resp.json::<TResp>().await?)
    }

    pub async fn post_json_raw<TReq: Serialize>(
        &self,
        path: &str,
        body: &TReq,
    ) -> Result<Response, ProviderError> {
        let url = self.build_url(path);
        let resp = self
            .http
            .request(Method::POST, url)
            .headers(self.build_headers())
            .json(body)
            .send()
            .await?;
        Ok(resp)
    }

    pub async fn post_multipart(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<Response, ProviderError> {
        let url = self.build_url(path);
        let resp = self
            .http
            .request(Method::POST, url)
            .headers(self.build_headers())
            .multipart(form)
            .send()
            .await?;
        Ok(resp)
    }

    pub async fn get_json<TResp: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<TResp, ProviderError> {
        let url = self.build_url(path);
        let resp = self
            .http
            .request(Method::GET, url)
            .headers(self.build_headers())
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(map_error_response(resp).await);
        }
        Ok(resp.json::<TResp>().await?)
    }
}

pub async fn map_error_response(resp: Response) -> ProviderError {
    let status = resp.status();
    match resp.text().await {
        Ok(body) => {
            // Try to pull a message from common JSON error shapes
            let message = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("error").cloned())
                .and_then(|e| e.get("message").cloned())
                .and_then(|m| m.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| body.clone());

            match status.as_u16() {
                401 => ProviderError::InvalidApiKey,
                404 => ProviderError::ModelNotFound {
                    model: "unknown".to_string(),
                },
                429 => ProviderError::RateLimit,
                code => ProviderError::Api { code, message },
            }
        }
        Err(_) => ProviderError::Api {
            code: status.as_u16(),
            message: "Failed to read error response".to_string(),
        },
    }
}
