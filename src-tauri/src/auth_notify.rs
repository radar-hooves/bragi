//! Deduplicated Jellyfin session-expiry signalling.
//!
//! When an already-authenticated session call (library, sync, artwork, or the
//! audio-stream proxy) receives a 401, the token has expired mid-session. A
//! single sync fans out up to `CONCURRENT_FETCH_LIMIT` album fetches, so a
//! naive "emit on every 401" would storm the frontend with duplicate events.
//!
//! [`AuthExpiredGuard`] is a shared one-shot latch: the *first* 401 after a
//! (re)connect emits exactly one `jellyfin-auth-expired` event; subsequent 401s
//! are swallowed until [`AuthExpiredGuard::reset`] re-arms it on a successful
//! reconnect. The guard is cloned (its `Arc` shared) into the Jellyfin client,
//! the proxy, and `AppState`, so all three signal — and reconnect resets — the
//! same latch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The Tauri event name the frontend listens on to surface the reconnect prompt.
pub const AUTH_EXPIRED_EVENT: &str = "jellyfin-auth-expired";

/// A shared one-shot latch that lets exactly one caller "win" per arming.
///
/// Cheap to clone — clones share the same underlying [`AtomicBool`].
#[derive(Clone, Default)]
pub struct AuthExpiredGuard(Arc<AtomicBool>);

impl AuthExpiredGuard {
    /// Create a fresh, armed guard (nothing has claimed it yet).
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Attempt to claim the latch. Returns `true` for exactly one caller
    /// between arming and [`reset`](Self::reset); every other caller (including
    /// concurrent ones) gets `false`.
    pub fn claim(&self) -> bool {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Re-arm the latch so the next `claim` can win again (called on reconnect).
    pub fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Payload for the `jellyfin-auth-expired` event.
///
/// Intentionally minimal: the frontend only needs to know a reconnect is
/// required. The notifier has no handle on the current server URL, so no URL is
/// threaded through.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AuthExpiredPayload {
    message: String,
}

/// Emits a single deduplicated session-expiry event via the app handle.
///
/// Cloned freely into callbacks; all clones share one [`AuthExpiredGuard`].
#[derive(Clone)]
pub struct AuthExpiryNotifier {
    app: AppHandle,
    guard: AuthExpiredGuard,
}

impl AuthExpiryNotifier {
    /// Build a notifier bound to `app`, gated by the shared `guard`.
    pub fn new(app: AppHandle, guard: AuthExpiredGuard) -> Self {
        Self { app, guard }
    }

    /// Emit the reconnect event — at most once per arming of the guard.
    pub fn notify(&self) {
        if self.guard.claim() {
            let payload = AuthExpiredPayload {
                message: "Jellyfin session expired".to_string(),
            };
            if let Err(e) = self.app.emit(AUTH_EXPIRED_EVENT, payload) {
                // Re-arm the latch so a later 401 can retry the emit rather than
                // being silently deduped away after a one-off emit failure.
                self.guard.reset();
                tracing::warn!("Failed to emit auth-expired event: {}", e);
            } else {
                tracing::info!("Jellyfin session expired; prompting reconnect");
            }
        }
    }

    /// A `Send + Sync` callback that fires [`notify`](Self::notify).
    ///
    /// Handed to the Jellyfin client and proxy, which have no `AppHandle` of
    /// their own but must be able to signal expiry from a 401.
    pub fn callback(&self) -> Arc<dyn Fn() + Send + Sync> {
        let notifier = self.clone();
        Arc::new(move || notifier.notify())
    }
}

#[cfg(test)]
mod tests {
    use super::AuthExpiredGuard;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Only the first claim wins; the latch stays claimed until reset.
    #[test]
    fn guard_claims_exactly_once() {
        let g = AuthExpiredGuard::new();
        assert!(g.claim(), "first claim must win");
        assert!(!g.claim(), "second claim must lose");
        assert!(!g.claim(), "further claims must lose");
    }

    /// Concurrent 401s (as produced by a fan-out sync hitting an expired token)
    /// must yield exactly one winner — no event storm.
    #[test]
    fn guard_is_single_winner_under_concurrency() {
        let g = AuthExpiredGuard::new();
        let winners = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..32 {
            let g = g.clone();
            let winners = winners.clone();
            handles.push(std::thread::spawn(move || {
                if g.claim() {
                    winners.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(
            winners.load(Ordering::SeqCst),
            1,
            "exactly one concurrent claimant may win"
        );
    }

    /// Reset re-arms the latch so a later expiry (after reconnect) fires again.
    #[test]
    fn reset_rearms_the_guard() {
        let g = AuthExpiredGuard::new();
        assert!(g.claim());
        assert!(!g.claim());
        g.reset();
        assert!(g.claim(), "reset must let the next claim win exactly once");
        assert!(!g.claim());
    }
}
