//! LLM provider layer for AI Playlists.
//!
//! A provider-agnostic client (Ollama / OpenAI-compatible / Anthropic) that
//! curates playlists from local library metadata. Keys live in
//! the keyring; the frontend only ever calls the `commands::ai_playlist`
//! wrappers, never these types directly.

pub mod catalogue;
pub mod client;
pub mod credentials;
pub mod migrate;
pub mod parse;
pub mod prompt;
pub mod provider;
pub mod resolve;

pub use client::LlmClient;
pub use parse::extract_json;
pub use provider::{LlmConfig, Provider};
