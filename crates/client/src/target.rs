//! Where calls go: a gateway, or a provider directly.

use std::fmt;

use ultrafast_translate::provider::{gateway_base, ProviderKind};

const REDACTED: &str = "[redacted]";

#[derive(Clone)]
pub enum Target {
    /// An Ultrafast gateway: OpenAI's wire format under `{base_url}/v1`.
    Gateway { base_url: String, key: String },
    Provider {
        kind: ProviderKind,
        base_url: String,
        key: String,
        api_version: Option<String>,
    },
}

impl Target {
    /// `base_url` is the gateway's address; a trailing `/v1` is accepted and ignored.
    pub fn gateway(base_url: impl Into<String>, key: impl Into<String>) -> Self {
        Target::Gateway {
            base_url: base_url.into(),
            key: key.into(),
        }
    }

    pub fn openai(key: impl Into<String>) -> Self {
        Self::provider(ProviderKind::OpenAi, "https://api.openai.com/v1", key)
    }

    pub fn anthropic(key: impl Into<String>) -> Self {
        Self::provider(ProviderKind::Anthropic, "https://api.anthropic.com", key)
    }

    pub fn gemini(key: impl Into<String>) -> Self {
        Self::provider(
            ProviderKind::Gemini,
            "https://generativelanguage.googleapis.com",
            key,
        )
    }

    /// `endpoint` is the resource URL; the request's model is the deployment.
    pub fn azure(endpoint: impl Into<String>, key: impl Into<String>) -> Self {
        Self::provider(ProviderKind::Azure, endpoint, key)
    }

    /// Any API that speaks OpenAI's format (Groq, Mistral, OpenRouter, Ollama);
    /// `base_url` includes the version segment, e.g. `https://host/v1`.
    pub fn openai_compatible(base_url: impl Into<String>, key: impl Into<String>) -> Self {
        Self::provider(ProviderKind::OpenAi, base_url, key)
    }

    fn provider(kind: ProviderKind, base_url: impl Into<String>, key: impl Into<String>) -> Self {
        Target::Provider {
            kind,
            base_url: base_url.into(),
            key: key.into(),
            api_version: None,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        match &mut self {
            Target::Gateway { base_url, .. } | Target::Provider { base_url, .. } => {
                *base_url = url.into();
            }
        }
        self
    }

    /// Azure only; the other kinds ignore it.
    pub fn with_api_version(mut self, version: impl Into<String>) -> Self {
        if let Target::Provider { api_version, .. } = &mut self {
            *api_version = Some(version.into());
        }
        self
    }

    pub(crate) fn key(&self) -> &str {
        match self {
            Target::Gateway { key, .. } | Target::Provider { key, .. } => key,
        }
    }

    pub(crate) fn is_gateway(&self) -> bool {
        matches!(self, Target::Gateway { .. })
    }

    /// The `translate` target for a call to `model`. A gateway is an
    /// OpenAI-kind target under `/v1`.
    pub(crate) fn translate(&self, model: &str) -> ultrafast_translate::provider::Target {
        let key = |k: &str| (!k.is_empty()).then(|| k.to_string());
        match self {
            Target::Gateway { base_url, key: k } => ultrafast_translate::provider::Target {
                kind: ProviderKind::OpenAi,
                base_url: gateway_base(base_url),
                api_key: key(k),
                model: model.to_string(),
                api_version: None,
            },
            Target::Provider {
                kind,
                base_url,
                key: k,
                api_version,
            } => ultrafast_translate::provider::Target {
                kind: *kind,
                base_url: base_url.clone(),
                api_key: key(k),
                model: model.to_string(),
                api_version: api_version.clone(),
            },
        }
    }

    pub(crate) fn kind(&self) -> ProviderKind {
        match self {
            Target::Gateway { .. } => ProviderKind::OpenAi,
            Target::Provider { kind, .. } => *kind,
        }
    }
}

/// Never prints the key.
impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Gateway { base_url, .. } => f
                .debug_struct("Gateway")
                .field("base_url", base_url)
                .field("key", &REDACTED)
                .finish(),
            Target::Provider {
                kind,
                base_url,
                api_version,
                ..
            } => f
                .debug_struct("Provider")
                .field("kind", kind)
                .field("base_url", base_url)
                .field("key", &REDACTED)
                .field("api_version", api_version)
                .finish(),
        }
    }
}
