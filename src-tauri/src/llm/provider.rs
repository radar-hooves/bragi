//! LLM provider identity + configuration.
//!
//! Three wire formats, one HTTP code path each. See `client.rs`.

use serde::{Deserialize, Serialize};

/// A supported LLM provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Local Ollama server — native `POST /api/chat`, no auth.
    Ollama,
    /// OpenAI or any OpenAI-compatible endpoint — `POST /v1/chat/completions`.
    OpenAi,
    /// Anthropic Claude — `POST /v1/messages`.
    Anthropic,
}

impl Provider {
    /// Parse from the lowercase settings string; `None` if unknown.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ollama" => Some(Self::Ollama),
            "openai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }

    /// The lowercase settings string for this provider.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
        }
    }

    /// Whether this provider authenticates with an API key. Ollama does not.
    pub fn needs_api_key(self) -> bool {
        !matches!(self, Self::Ollama)
    }

    /// The default model when the user has not chosen one.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::Ollama => "llama3.2",
            Self::OpenAi => "gpt-4o-mini",
            Self::Anthropic => "claude-haiku-4-5",
        }
    }

    /// The default endpoint base for this provider; a user-set base URL
    /// overrides it (an OpenAI-compatible server other than OpenAI's own).
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Ollama => "http://localhost:11434",
            Self::OpenAi => "https://api.openai.com",
            Self::Anthropic => "https://api.anthropic.com",
        }
    }
}

/// Non-secret LLM configuration resolved from settings (the API key is loaded
/// separately from the keyring and never travels with this struct).
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: Provider,
    pub model: String,
    /// User-set base URL; empty means "use the provider default".
    pub base_url: String,
    pub max_tokens: u32,
}

impl LlmConfig {
    /// The effective model (falls back to the provider default when unset).
    pub fn effective_model(&self) -> String {
        if self.model.trim().is_empty() {
            self.provider.default_model().to_string()
        } else {
            self.model.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trips_all_providers() {
        for p in [Provider::Ollama, Provider::OpenAi, Provider::Anthropic] {
            assert_eq!(Provider::parse(p.as_str()), Some(p));
        }
        assert_eq!(Provider::parse("nope"), None);
    }

    #[test]
    fn only_ollama_skips_api_key() {
        assert!(!Provider::Ollama.needs_api_key());
        assert!(Provider::OpenAi.needs_api_key());
        assert!(Provider::Anthropic.needs_api_key());
    }

    #[test]
    fn effective_model_falls_back_to_default() {
        let cfg = LlmConfig {
            provider: Provider::Anthropic,
            model: String::new(),
            base_url: String::new(),
            max_tokens: 2048,
        };
        assert_eq!(cfg.effective_model(), "claude-haiku-4-5");

        let custom = LlmConfig {
            model: "claude-opus-4-8".to_string(),
            ..cfg
        };
        assert_eq!(custom.effective_model(), "claude-opus-4-8");
    }
}
