//! LLM HTTP client.
//!
//! One client that speaks the three LLM wire formats behind enum dispatch,
//! mirroring `MusicServiceClient` (which speaks a scrobble server's own API and
//! ListenBrainz's). Auth differs per provider, so — unlike that client — no
//! default auth header is set; each path attaches its own.

use std::time::Duration;

use reqwest_middleware::{ClientWithMiddleware, RequestBuilder};
use serde_json::{json, Value};
use url::Url;

use super::provider::{LlmConfig, Provider};
use crate::error::AppError;

const CLOUD_TIMEOUT: Duration = Duration::from_secs(120);
/// Local models are slow to load and generate.
const OLLAMA_TIMEOUT: Duration = Duration::from_secs(300);
/// Temperature for playlist curation — a little variety, still coherent.
const TEMPERATURE: f32 = 0.7;

/// A configured LLM client for a single provider.
pub struct LlmClient {
    provider: Provider,
    /// Endpoint base with no trailing slash.
    base_url: Url,
    model: String,
    api_key: String,
    max_tokens: u32,
    http: ClientWithMiddleware,
}

impl LlmClient {
    /// Build a client from resolved config + the API key (already loaded from
    /// the keyring by the caller).
    pub fn new(config: &LlmConfig, api_key: String) -> Result<Self, AppError> {
        let base_url = resolve_base_url(config)?;
        let timeout = if config.provider == Provider::Ollama {
            OLLAMA_TIMEOUT
        } else {
            CLOUD_TIMEOUT
        };
        let http = crate::http::client(crate::http::default_headers(), timeout);

        Ok(Self {
            provider: config.provider,
            base_url,
            model: config.effective_model(),
            api_key,
            max_tokens: config.max_tokens,
            http,
        })
    }

    /// Send a system + user prompt and return the raw assistant text. When a
    /// `schema` is given, request structured JSON output where the provider
    /// supports it (the caller still parses defensively).
    pub async fn complete(
        &self,
        system: &str,
        user: &str,
        schema: Option<&Value>,
    ) -> Result<String, AppError> {
        match self.provider {
            Provider::Ollama => self.complete_ollama(system, user, schema).await,
            Provider::OpenAi => self.complete_openai(system, user, schema).await,
            Provider::Anthropic => self.complete_anthropic(system, user, schema).await,
        }
    }

    /// Probe the provider (reachability + auth) AND enumerate its available
    /// models in a single round trip — the model-list endpoint doubles as the
    /// health check, since an unreachable host, a bad key, or an unauthenticated
    /// proxy all fail the same call. The settings UI feeds the returned list into
    /// a model dropdown for the user to choose from.
    pub async fn list_models(&self) -> Result<Vec<String>, AppError> {
        match self.provider {
            Provider::Ollama => self.list_models_ollama().await,
            Provider::OpenAi => self.list_models_openai().await,
            Provider::Anthropic => self.list_models_anthropic().await,
        }
    }

    async fn list_models_ollama(&self) -> Result<Vec<String>, AppError> {
        // GET /api/tags — no auth; lists the models the server has pulled.
        let data = self.run(self.http.get(self.join("api/tags")?)).await?;
        let mut models = json_array_strings(&data["models"], &["name", "model"]);
        models.sort();
        Ok(models)
    }

    async fn list_models_openai(&self) -> Result<Vec<String>, AppError> {
        // GET /v1/models — covers OpenAI and any OpenAI-compatible server.
        let req = self
            .http
            .get(self.join("v1/models")?)
            .bearer_auth(&self.api_key);
        let data = self.run(req).await?;
        let mut models = json_array_strings(&data["data"], &["id"]);
        models.sort();
        Ok(models)
    }

    async fn list_models_anthropic(&self) -> Result<Vec<String>, AppError> {
        // GET /v1/models with the messages-API headers; fall back to a curated
        // list on older keys that 404 the models endpoint.
        let req = self
            .http
            .get(self.join("v1/models")?)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01");
        match self.run(req).await {
            Ok(data) => {
                let mut models = json_array_strings(&data["data"], &["id"]);
                if models.is_empty() {
                    models = fallback_anthropic_models();
                }
                models.sort();
                Ok(models)
            }
            Err(AppError::NotFound(_)) => Ok(fallback_anthropic_models()),
            Err(e) => Err(e),
        }
    }

    // ── OpenAI and OpenAI-compatible ────────────────────────────────────────
    async fn complete_openai(
        &self,
        system: &str,
        user: &str,
        schema: Option<&Value>,
    ) -> Result<String, AppError> {
        let url = self.join("v1/chat/completions")?;
        let mut body = json!({
            "model": self.model,
            "messages": self.chat_messages(system, user),
            "max_tokens": self.max_tokens,
            "temperature": TEMPERATURE,
        });
        if let Some(schema) = schema {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": "bragi_playlist", "strict": true, "schema": schema },
            });
        }
        let req = self.http.post(url).bearer_auth(&self.api_key).json(&body);
        let data = self.run(req).await?;
        data["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| AppError::InvalidOperation("Unexpected LLM response shape".into()))
    }

    // ── Ollama ────────────────────────────────────────────────────────────────
    async fn complete_ollama(
        &self,
        system: &str,
        user: &str,
        schema: Option<&Value>,
    ) -> Result<String, AppError> {
        let url = self.join("api/chat")?;
        let mut body = json!({
            "model": self.model,
            "messages": self.chat_messages(system, user),
            "stream": false,
            "options": { "temperature": TEMPERATURE, "num_predict": self.max_tokens },
        });
        if let Some(schema) = schema {
            // Ollama accepts a JSON Schema (or "json") in the top-level format field.
            body["format"] = schema.clone();
        }
        let req = self.http.post(url).json(&body);
        let data = self.run(req).await?;
        data["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| AppError::InvalidOperation("Unexpected Ollama response shape".into()))
    }

    // ── Anthropic ─────────────────────────────────────────────────────────────
    async fn complete_anthropic(
        &self,
        system: &str,
        user: &str,
        schema: Option<&Value>,
    ) -> Result<String, AppError> {
        let url = self.join("v1/messages")?;
        // Anthropic takes `system` as a top-level field, not a message role.
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "temperature": TEMPERATURE,
            "messages": [{ "role": "user", "content": user }],
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        if let Some(schema) = schema {
            // Structured output via a single forced tool.
            body["tools"] = json!([{
                "name": "emit_playlist",
                "description": "Return the playlist as structured JSON.",
                "input_schema": schema,
            }]);
            body["tool_choice"] = json!({ "type": "tool", "name": "emit_playlist" });
        }
        let req = self
            .http
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body);
        let data = self.run(req).await?;

        // When a tool was forced, the JSON is the tool_use block's `input`.
        if schema.is_some() {
            if let Some(input) = data["content"]
                .as_array()
                .and_then(|blocks| blocks.iter().find(|b| b["type"] == "tool_use"))
                .map(|b| &b["input"])
            {
                return Ok(input.to_string());
            }
        }
        data["content"][0]["text"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| AppError::InvalidOperation("Unexpected Anthropic response shape".into()))
    }

    // ── helpers ────────────────────────────────────────────────────────────────
    fn chat_messages(&self, system: &str, user: &str) -> Value {
        let mut messages = Vec::new();
        if !system.is_empty() {
            messages.push(json!({ "role": "system", "content": system }));
        }
        messages.push(json!({ "role": "user", "content": user }));
        Value::Array(messages)
    }

    /// Join a path onto the base URL (base has no trailing slash).
    fn join(&self, path: &str) -> Result<Url, AppError> {
        let full = format!("{}/{}", self.base_url.as_str().trim_end_matches('/'), path);
        Url::parse(&full).map_err(|e| AppError::Config(format!("Invalid LLM URL '{full}': {e}")))
    }

    /// Send a request, mapping transport + status failures to distinct errors,
    /// and return the parsed JSON body.
    async fn run(&self, req: RequestBuilder) -> Result<Value, AppError> {
        let resp = req.send().await.map_err(|e| {
            let (timed_out, connect) = match &e {
                reqwest_middleware::Error::Reqwest(r) => (r.is_timeout(), r.is_connect()),
                _ => (false, false),
            };
            if timed_out {
                AppError::Network("LLM request timed out".into())
            } else if connect {
                AppError::Network(format!(
                    "LLM endpoint not reachable at {}: {e}",
                    self.base_url
                ))
            } else {
                AppError::Network(format!("LLM request failed: {e}"))
            }
        })?;
        let status = resp.status();
        if !status.is_success() {
            // Include the response body so provider-specific reasons (bad model,
            // "use max_completion_tokens", quota, etc.) reach the user.
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.trim().chars().take(300).collect();
            return Err(match status.as_u16() {
                401 | 403 => AppError::Auth("Invalid API key".into()),
                429 => AppError::Network("LLM rate limited (429) — try again shortly".into()),
                _ if snippet.is_empty() => map_status(status),
                _ => AppError::Network(format!("LLM returned {status}: {snippet}")),
            });
        }
        resp.json::<Value>()
            .await
            .map_err(|e| AppError::InvalidOperation(format!("Failed to read LLM response: {e}")))
    }
}

/// Resolve the effective base URL: the user's, else the provider default. A
/// user-supplied trailing version segment (e.g. `.../v1`, as OpenAI-compatible
/// providers document their base URL) is stripped, since the wire paths already
/// include it — otherwise we'd request `.../v1/v1/...`.
fn resolve_base_url(config: &LlmConfig) -> Result<Url, AppError> {
    let mut raw = if config.base_url.trim().is_empty() {
        config.provider.default_base_url().to_string()
    } else {
        config.base_url.trim().trim_end_matches('/').to_string()
    };
    if let Some((head, last)) = raw.rsplit_once('/') {
        let is_version = last
            .strip_prefix('v')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if is_version {
            raw = head.to_string();
        }
    }
    Url::parse(&raw).map_err(|e| AppError::Config(format!("Invalid LLM base URL '{raw}': {e}")))
}

/// Map a non-2xx status to a user-actionable error.
fn map_status(status: reqwest::StatusCode) -> AppError {
    match status.as_u16() {
        401 | 403 => AppError::Auth("Invalid API key".into()),
        404 => AppError::NotFound(format!("Model or endpoint not found ({status})")),
        429 => AppError::Network("LLM rate limited (429) — try again shortly".into()),
        _ => AppError::Network(format!("LLM returned status {status}")),
    }
}

/// Collect string values from a JSON array, reading the first matching key from
/// each element (e.g. Ollama `name`/`model`, OpenAI `id`).
fn json_array_strings(value: &Value, keys: &[&str]) -> Vec<String> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|el| {
                    keys.iter()
                        .find_map(|k| el[*k].as_str())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A curated Anthropic model list, used only if the live `/v1/models` endpoint
/// is unavailable (older keys).
fn fallback_anthropic_models() -> Vec<String> {
    ["claude-haiku-4-5", "claude-sonnet-5", "claude-opus-4-8"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
