//! AI Playlists commands.
//!
//! Provider config + connection (`test_llm_connection` returns the model list),
//! and generation (`generate_ai_playlist`: prompt → candidate catalogue → LLM →
//! resolve → draft). API keys are set/cleared here (stored in the keyring, never
//! returned to the frontend); non-secret config lives in settings.

use crate::db::settings::keys;
use crate::db::Database;
use crate::error::{CommandError, CommandResult};
use crate::llm::catalogue::{self, PlaylistIntent};
use crate::llm::{credentials, extract_json, prompt, provider::Provider, resolve};
use crate::llm::{LlmClient, LlmConfig};
use crate::models::Track;
use crate::state::AppState;

/// Resolve the persisted (non-secret) LLM config from settings.
fn resolve_llm_config(db: &Database) -> Result<LlmConfig, String> {
    let provider = db
        .get_setting(keys::LLM_PROVIDER)?
        .and_then(|v| Provider::parse(&v))
        .unwrap_or(Provider::Ollama);
    let model = db.get_setting(keys::LLM_MODEL)?.unwrap_or_default();
    let base_url = db.get_setting(keys::LLM_BASE_URL)?.unwrap_or_default();
    let max_tokens = db
        .get_setting(keys::LLM_MAX_TOKENS)?
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(2048);

    Ok(LlmConfig {
        provider,
        model,
        base_url,
        max_tokens,
    })
}

/// Store the API key for a provider in the keyring.
#[tauri::command]
pub async fn set_llm_api_key(provider: String, api_key: String) -> CommandResult<()> {
    let p =
        Provider::parse(&provider).ok_or_else(|| format!("Unknown LLM provider: {provider}"))?;
    credentials::set_key(p, Some(api_key)).map_err(CommandError::msg)
}

/// Clear the stored API key for a provider.
#[tauri::command]
pub async fn clear_llm_api_key(provider: String) -> CommandResult<()> {
    let p =
        Provider::parse(&provider).ok_or_else(|| format!("Unknown LLM provider: {provider}"))?;
    credentials::set_key(p, None).map_err(CommandError::msg)
}

/// Result of a connection test: reachability/auth outcome PLUS the list of
/// available models the settings UI populates its dropdown from. Failures are
/// returned here (ok=false + message), not as command errors, so the UI can
/// render the reason inline.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestLlmConnectionResult {
    pub ok: bool,
    pub models: Vec<String>,
    pub message: String,
}

/// Test a provider connection using the settings the user is editing (falling
/// back to persisted values), and return the available models in one call.
///
/// The model-list endpoint (`/api/tags`, `/v1/models`) doubles as the
/// reachability + auth probe: an unreachable host, a bad key, or an
/// unauthenticated proxy all fail the same call that would enumerate models.
#[tauri::command]
pub async fn test_llm_connection(
    state: tauri::State<'_, AppState>,
    provider: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> CommandResult<TestLlmConnectionResult> {
    let persisted = resolve_llm_config(&state.db)?;
    let prov = provider
        .as_deref()
        .and_then(Provider::parse)
        .unwrap_or(persisted.provider);

    let config = LlmConfig {
        provider: prov,
        model: persisted.model,
        base_url: base_url.unwrap_or(persisted.base_url),
        max_tokens: persisted.max_tokens,
    };

    // The key typed into the form wins; else the stored key.
    let key = match api_key {
        Some(k) if !k.is_empty() => k,
        _ => credentials::resolve_key(prov)?,
    };
    if prov.needs_api_key() && key.is_empty() {
        return Ok(TestLlmConnectionResult {
            ok: false,
            models: vec![],
            message: format!("No API key set for {}", prov.as_str()),
        });
    }

    let client = match LlmClient::new(&config, key) {
        Ok(c) => c,
        Err(e) => {
            return Ok(TestLlmConnectionResult {
                ok: false,
                models: vec![],
                message: e.to_string(),
            })
        }
    };

    match client.list_models().await {
        Ok(models) => {
            let message = if models.is_empty() {
                format!("Connected to {} (no models reported)", prov.as_str())
            } else {
                format!(
                    "Connected \u{b7} {} model{} available",
                    models.len(),
                    if models.len() == 1 { "" } else { "s" }
                )
            };
            Ok(TestLlmConnectionResult {
                ok: true,
                models,
                message,
            })
        }
        Err(e) => Ok(TestLlmConnectionResult {
            ok: false,
            models: vec![],
            message: e.to_string(),
        }),
    }
}

/// A generated playlist the user reviews before saving. Not persisted until the
/// frontend saves it as an ordinary Jellyfin playlist.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiPlaylistDraft {
    /// The name the model suggested (falls back to the prompt).
    pub suggested_name: String,
    /// The resolved library tracks, in play order.
    pub tracks: Vec<Track>,
    /// How many tracks the model proposed.
    pub suggested_count: u32,
    /// How many resolved to real library tracks (drives "matched N of M").
    pub matched_count: u32,
    /// The prompt that produced this draft (for the refine flow).
    pub prompt: String,
}

/// The model's structured playlist response.
#[derive(serde::Deserialize)]
struct PlaylistResponse {
    #[serde(default)]
    name: String,
    #[serde(default)]
    indices: Vec<i64>,
}

/// Order the pool by play counts from the user's configured source (`auto` =
/// Jellyfin then the scrobble server; `jellyfin`/`earworm` pin one), or shuffle for `none`.
/// Jellyfin counts are fetched on-demand; any failure degrades gracefully.
async fn order_pool_by_play_counts(
    state: &tauri::State<'_, AppState>,
    pool: &mut [Track],
    sort: &str,
) -> Result<(), String> {
    if !catalogue::sort_needs_play_data(sort) {
        catalogue::order_pool(pool, sort, |_| catalogue::PlayMetric::default());
        return Ok(());
    }

    let source = state
        .db
        .get_setting(keys::PLAY_COUNT_SOURCE)
        .ok()
        .flatten()
        .unwrap_or_else(|| "auto".to_string());

    // Scrobble-server stats: a cheap local query; used for the earworm/auto tiers and
    // for recency ordering regardless of source.
    let earworm = state.db.get_all_track_stats()?;

    // Jellyfin play counts (on-demand) unless the user pinned the scrobble server, or the
    // pool is so large (an unfiltered "my most played" over a big library) that
    // fetching every count in serial chunks would hang — fall back to the scrobble server.
    const MAX_JELLYFIN_PLAY_COUNT_FETCH: usize = 1000;
    let jellyfin = if source == "earworm" || pool.len() > MAX_JELLYFIN_PLAY_COUNT_FETCH {
        std::collections::HashMap::new()
    } else {
        let ids: Vec<String> = pool.iter().map(|t| t.jellyfin_id.clone()).collect();
        // Clone the client and drop the lock before awaiting (release-before-await).
        let client = {
            let guard = state.media.read().await;
            guard.as_ref().cloned()
        };
        match client {
            Some(c) => c.get_track_play_counts(&ids).await.unwrap_or_default(),
            None => std::collections::HashMap::new(),
        }
    };

    catalogue::order_pool(pool, sort, |t| {
        let jf = jellyfin.get(&t.jellyfin_id).copied();
        let ew = t.mbid.as_deref().and_then(|m| earworm.get(m));
        let play_count = match source.as_str() {
            "jellyfin" => jf.unwrap_or(0) as i64,
            "earworm" => ew.map(|s| s.play_count).unwrap_or(0) as i64,
            // auto: Jellyfin if it recorded any plays, else the scrobble server, else 0.
            _ => jf
                .filter(|&c| c > 0)
                .map(|c| c as i64)
                .or_else(|| ew.map(|s| s.play_count as i64))
                .unwrap_or(0),
        };
        let last_played = ew.map(|s| s.last_played_at).unwrap_or(0);
        catalogue::PlayMetric {
            play_count,
            last_played,
        }
    });
    Ok(())
}

/// Generate a playlist draft from a natural-language prompt: build a candidate
/// catalogue from the local library, have the LLM pick tracks by index, and
/// resolve those indices back to real tracks. Only metadata is sent — never
/// audio — and the result is a draft the user explicitly saves.
#[tauri::command]
pub async fn generate_ai_playlist(
    state: tauri::State<'_, AppState>,
    prompt: String,
) -> CommandResult<AiPlaylistDraft> {
    tauri_plugin_telemetry::traced("generate_ai_playlist", async move {
        let request = prompt.trim().to_string();
        if request.is_empty() {
            return Err("Enter a prompt describing the playlist you want.".into());
        }

        // Respect the feature toggle even if the UI is bypassed.
        let enabled = state
            .db
            .get_setting(keys::AI_PLAYLIST_ENABLED)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);
        if !enabled {
            return Err(
                "AI playlists are disabled. Enable them in Settings \u{2192} AI Playlists.".into(),
            );
        }

        let config = resolve_llm_config(&state.db)?;
        let key = credentials::resolve_key(config.provider)?;
        if config.provider.needs_api_key() && key.is_empty() {
            return Err(format!(
                "No API key set for {}. Configure it in Settings \u{2192} AI Playlists.",
                config.provider.as_str()
            )
            .into());
        }

        let client = LlmClient::new(&config, key)?;

        // ── Pass 1: understand the request as a structured query. ──────────────────
        let genres = state.db.distinct_genres()?;
        let intent_raw = client
            .complete(
                &prompt::intent_system_prompt(&genres),
                &request,
                Some(&prompt::intent_schema()),
            )
            .await
            .inspect_err(|e| telemetry::report_error_with_cause("AI playlist failure", e))?;
        // A parse failure degrades to an empty intent (a random library sample)
        // rather than failing the whole request — but is logged, not silent.
        let intent: PlaylistIntent = match extract_json(&intent_raw) {
            Ok(value) => serde_json::from_value(value).unwrap_or_else(|e| {
                tracing::warn!("AI playlist intent parse failed ({e}); using an empty intent");
                PlaylistIntent::default()
            }),
            Err(e) => {
                tracing::warn!(
                    "AI playlist intent JSON extraction failed ({e}); using an empty intent"
                );
                PlaylistIntent::default()
            }
        };

        // Execute the query: filter by artist/genre/year.
        let mut pool = catalogue::select_tracks(&state.db, &intent)?;
        if pool.is_empty() {
            return Err(
            "Nothing in your library matched that request \u{2014} try different artists or genres."
                .into(),
        );
        }

        // Order the pool. Play-count sorts draw on the user's configured source
        // (auto = Jellyfin then the scrobble server); "none" shuffles.
        order_pool_by_play_counts(&state, &mut pool, &intent.sort).await?;

        let count = intent.count.unwrap_or(25).clamp(1, 100) as usize;
        let base_name = if intent.name.trim().is_empty() {
            request.clone()
        } else {
            intent.name.trim().to_string()
        };

        let (tracks, suggested_name, suggested_count) = if intent.curate {
            // ── Pass 2 (vibe only): let the model pick from the filtered pool. ─────
            if pool.len() > catalogue::MAX_CANDIDATES {
                pool.truncate(catalogue::MAX_CANDIDATES);
            }
            let custom = state
                .db
                .get_setting(keys::LLM_SYSTEM_PROMPT)
                .ok()
                .flatten()
                .unwrap_or_default();
            let raw = client
                .complete(
                    &prompt::system_prompt(&custom),
                    &prompt::user_message(&request, &prompt::candidate_list(&pool)),
                    Some(&prompt::response_schema()),
                )
                .await
                .inspect_err(|e| telemetry::report_error_with_cause("AI playlist failure", e))?;
            let value = extract_json(&raw)?;
            // Accept {name, indices} or a bare index array.
            let parsed: PlaylistResponse = if value.is_array() {
                PlaylistResponse {
                    name: String::new(),
                    indices: serde_json::from_value(value)
                        .map_err(|e| format!("Could not read the model's track list: {e}"))?,
                }
            } else {
                serde_json::from_value(value)
                    .map_err(|e| format!("Could not read the model's playlist: {e}"))?
            };
            let resolution = resolve::resolve_indices(&pool, &parsed.indices);
            let name = if parsed.name.trim().is_empty() {
                base_name
            } else {
                parsed.name.trim().to_string()
            };
            (resolution.tracks, name, resolution.suggested_count as u32)
        } else {
            // ── Precise path: take the top N of the already-ordered pool. ──────────
            let n = count.min(pool.len());
            let tracks: Vec<Track> = pool.into_iter().take(n).collect();
            let len = tracks.len() as u32;
            (tracks, base_name, len)
        };

        if tracks.is_empty() {
            return Err("That request didn't resolve to any tracks. Try rephrasing it.".into());
        }

        Ok(AiPlaylistDraft {
            suggested_name,
            matched_count: tracks.len() as u32,
            suggested_count,
            tracks,
            prompt: request,
        })
    })
    .await
}
