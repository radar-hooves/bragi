import { invoke } from '@tauri-apps/api/core';
import type { BackendConnection, ConnectionStatus, MediaBackendsStatus, QuickConnectInfo } from '#lib/types/tauri.js';

type BackendKind = 'jellyfin' | 'opensubsonic';

/**
 * Normalise a server URL entered by the user.
 *
 * Prepends `https://` when no scheme is present (bare hostnames like
 * `jellyfin.example.com`) and strips trailing slashes.
 */
export function normaliseServerUrl(raw: string): string {
  let url = raw.trim().replace(/\/+$/, '');
  if (url && !url.includes('://')) {
    url = `https://${url}`;
  }
  return url;
}

/**
 * Is `raw` (after normalisation) a cleartext `http://` connection to a
 * non-loopback host? Mirrors the Rust `is_cleartext_url`.
 *
 * Cleartext is still allowed (LAN parity), but credentials — for Subsonic a
 * non-expiring salted token — and all audio travel unencrypted, so the UI shows
 * a non-blocking warning. Loopback (`localhost`, `127.0.0.1`, `[::1]`) is exempt
 * (no network hop to sniff). A bare hostname normalises to `https://` → false.
 */
export function isCleartextServerUrl(raw: string): boolean {
  const normalised = normaliseServerUrl(raw);
  if (!normalised) return false;
  let parsed: URL;
  try {
    parsed = new URL(normalised);
  } catch {
    return false;
  }
  if (parsed.protocol !== 'http:') return false;
  const host = parsed.hostname.toLowerCase();
  return host !== 'localhost' && host !== '127.0.0.1' && host !== '[::1]' && host !== '::1';
}

/** Auth method tab for Jellyfin login */
export type JellyfinAuthTab = 'quick-connect' | 'password';

/** Form state for Jellyfin connection (persists across component lifecycle) */
interface JellyfinFormState {
  url: string;
  username: string;
  password: string;
  authTab: JellyfinAuthTab;
  isQuickConnectAvailable: boolean | null;
  error: string | null;
  testResult: boolean | null;
}

/** Jellyfin connection state */
interface JellyfinState {
  status: ConnectionStatus;
  isLoading: boolean;
  lastError: string | null;
  /** Quick Connect code displayed to the user (null when inactive). */
  quickConnectCode: string | null;
  /** Quick Connect secret used for polling (null when inactive). */
  quickConnectSecret: string | null;
  /**
   * The Jellyfin session expired mid-use (a 401 on a session call).
   * Drives the persistent "reconnect" prompt; cleared on a successful reconnect.
   */
  authExpired: boolean;
  /**
   * Per-backend connection status: whether each backend has stored
   * credentials, independent of which one is currently active. Drives the
   * dual-backend Connections UI (two cards + a live active-backend toggle).
   */
  backends: { jellyfin: BackendConnection; opensubsonic: BackendConnection };
  /** Form state for connection UI (persists across tab switches). */
  form: JellyfinFormState;
}

const initialFormState: JellyfinFormState = {
  url: '',
  username: '',
  password: '',
  authTab: 'quick-connect',
  isQuickConnectAvailable: null,
  error: null,
  testResult: null,
};

const initialState: JellyfinState = {
  status: {
    connected: false,
  },
  isLoading: false,
  lastError: null,
  quickConnectCode: null,
  quickConnectSecret: null,
  authExpired: false,
  backends: {
    jellyfin: { connected: false },
    opensubsonic: { connected: false },
  },
  form: { ...initialFormState },
};

let jellyfinState = $state<JellyfinState>({ ...initialState });

/**
 * Bumped every time connection identity is (re)confirmed from the backend —
 * after a connect, disconnect or backend switch, and by `initJellyfinState()`
 * on startup. A caller that starts a live Jellyfin fetch and applies
 * its result later (rather than awaiting it inline) reads this before the
 * fetch and compares after: a mismatch means the connection changed while the
 * fetch was in flight, so the result is stale and must not be applied — e.g.
 * playback starting against a backend the user has since switched away from
 * or disconnected. Not `$state`: nothing renders from it, it only needs to be
 * read at two points in time.
 */
let connectionGeneration = 0;

/** Current connection generation; see `connectionGeneration` above. */
export function getConnectionGeneration(): number {
  return connectionGeneration;
}

/** Get the Jellyfin state */
export function getJellyfinState(): JellyfinState {
  return jellyfinState;
}

/**
 * Whether a media server is connected. Backend-agnostic: true for either
 * Jellyfin or OpenSubsonic, so app-wide library/playback flows work for both.
 */
export function getIsJellyfinConnected(): boolean {
  return jellyfinState.status.connected;
}

/**
 * The currently-active backend token, or undefined when none is live.
 * Reactive on `status.serverType`; used to drive the toggle value and the
 * library-reload effect. A connection with no serverType is an older Jellyfin
 * session, treated as `jellyfin`.
 */
export function getActiveBackend(): BackendKind | undefined {
  if (!jellyfinState.status.connected) return undefined;
  return jellyfinState.status.serverType ?? 'jellyfin';
}

/** Whether OpenSubsonic is specifically the ACTIVE (live) backend. */
export function getIsSubsonicActive(): boolean {
  return getActiveBackend() === 'opensubsonic';
}

/** Whether Jellyfin is specifically the ACTIVE (live) backend. */
export function getIsJellyfinActive(): boolean {
  return getActiveBackend() === 'jellyfin';
}

/** Per-backend "configured" status: does this backend have stored creds? */
export function getJellyfinBackend(): BackendConnection {
  return jellyfinState.backends.jellyfin;
}

export function getSubsonicBackend(): BackendConnection {
  return jellyfinState.backends.opensubsonic;
}

/** Whether Jellyfin is independently connected (has stored creds). */
export function getIsJellyfinConfigured(): boolean {
  return jellyfinState.backends.jellyfin.connected;
}

/** Whether OpenSubsonic is independently connected (has stored creds). */
export function getIsSubsonicConfigured(): boolean {
  return jellyfinState.backends.opensubsonic.connected;
}

/** Whether the Jellyfin session has expired and needs a reconnect. */
export function getAuthExpired(): boolean {
  return jellyfinState.authExpired;
}

/** Set the session-expired flag (mutates the property directly per repo rule). */
export function setAuthExpired(value: boolean): void {
  jellyfinState.authExpired = value;
}

/**
 * Clear the expired state after a successful reconnect and, if we were expired,
 * resume syncing. Dismissing the persistent toast and importing
 * `syncLibrary` are done via dynamic import to avoid a static import cycle
 * between this store and the events service.
 */
export async function resumeAfterReconnect(): Promise<void> {
  const wasExpired = jellyfinState.authExpired;
  setAuthExpired(false);

  try {
    const { dismissAuthExpiredToast } = await import('#lib/services/events.js');
    dismissAuthExpiredToast();
  } catch (e) {
    console.warn('Failed to dismiss auth-expired toast:', e);
  }

  if (wasExpired) {
    try {
      const { syncLibrary } = await import('#lib/services/tauri.js');
      await syncLibrary();
    } catch (e) {
      console.warn('Resume sync after reconnect failed:', e);
    }
  }
}

/**
 * Log in to a Jellyfin server using username and password.
 */
export async function loginJellyfin(serverUrl: string, username: string, password: string): Promise<void> {
  jellyfinState.isLoading = true;
  jellyfinState.lastError = null;

  try {
    const url = normaliseServerUrl(serverUrl);
    await invoke('login_jellyfin', { serverUrl: url, username, password });
    await refreshJellyfinStatus();
    void resumeAfterReconnect();
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    jellyfinState.lastError = error;
    throw e;
  } finally {
    jellyfinState.isLoading = false;
  }
}

/**
 * Log in to an OpenSubsonic server (Navidrome, Gonic, Airsonic).
 *
 * Shares the single media-connection slot with Jellyfin: on success the app's
 * library and playback flows target the Subsonic backend via the MediaClient
 * enum, exactly as they do for Jellyfin.
 */
export async function loginSubsonic(serverUrl: string, username: string, password: string): Promise<void> {
  jellyfinState.isLoading = true;
  jellyfinState.lastError = null;

  try {
    const url = normaliseServerUrl(serverUrl);
    await invoke('login_subsonic', { serverUrl: url, username, password });
    await refreshJellyfinStatus();
    void resumeAfterReconnect();
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    jellyfinState.lastError = error;
    throw e;
  } finally {
    jellyfinState.isLoading = false;
  }
}

/**
 * Check whether Quick Connect is enabled on a Jellyfin server.
 */
export async function checkQuickConnect(serverUrl: string): Promise<boolean> {
  const url = normaliseServerUrl(serverUrl);
  if (!url) return false;
  return invoke<boolean>('quick_connect_check', { serverUrl: url });
}

/**
 * Initiate a Quick Connect session. Stores the code and secret in state.
 */
export async function startQuickConnect(serverUrl: string): Promise<QuickConnectInfo> {
  jellyfinState.isLoading = true;
  jellyfinState.lastError = null;

  try {
    const url = normaliseServerUrl(serverUrl);
    const info = await invoke<QuickConnectInfo>('quick_connect_start', { serverUrl: url });
    jellyfinState.quickConnectCode = info.code;
    jellyfinState.quickConnectSecret = info.secret;
    return info;
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    jellyfinState.lastError = error;
    throw e;
  } finally {
    jellyfinState.isLoading = false;
  }
}

/**
 * Poll a Quick Connect session. Returns true when authorised.
 */
export async function pollQuickConnect(serverUrl: string, secret: string): Promise<boolean> {
  const url = normaliseServerUrl(serverUrl);
  return invoke<boolean>('quick_connect_status', { serverUrl: url, secret });
}

/**
 * Complete Quick Connect authentication and connect.
 */
export async function completeQuickConnect(serverUrl: string, secret: string): Promise<void> {
  jellyfinState.isLoading = true;
  jellyfinState.lastError = null;

  try {
    const url = normaliseServerUrl(serverUrl);
    await invoke('quick_connect_complete', { serverUrl: url, secret });
    jellyfinState.quickConnectCode = null;
    jellyfinState.quickConnectSecret = null;
    await refreshJellyfinStatus();
    void resumeAfterReconnect();
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    jellyfinState.lastError = error;
    throw e;
  } finally {
    jellyfinState.isLoading = false;
  }
}

/**
 * Clear Quick Connect state without completing authentication.
 */
export function cancelQuickConnect(): void {
  jellyfinState.quickConnectCode = null;
  jellyfinState.quickConnectSecret = null;
  jellyfinState.lastError = null;
}

/**
 * Update Jellyfin form state. Partial updates are merged with existing form state.
 */
export function updateJellyfinForm(updates: Partial<JellyfinFormState>): void {
  Object.assign(jellyfinState.form, updates);
}

/**
 * Reset Jellyfin form state to initial values.
 */
export function resetJellyfinForm(): void {
  Object.assign(jellyfinState.form, initialFormState);
}

/**
 * Connect to a Jellyfin server using an API key.
 * @deprecated Use `loginJellyfin` or Quick Connect instead.
 */
export async function connectJellyfin(serverUrl: string, apiKey: string, userId: string): Promise<void> {
  jellyfinState.isLoading = true;
  jellyfinState.lastError = null;

  try {
    const url = normaliseServerUrl(serverUrl);
    await invoke('connect_jellyfin', { serverUrl: url, apiKey, userId });
    await refreshJellyfinStatus();
    void resumeAfterReconnect();
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    jellyfinState.lastError = error;
    throw e;
  } finally {
    jellyfinState.isLoading = false;
  }
}

/**
 * Clear the session-expired flag and dismiss the persistent reconnect toast.
 *
 * Call ONLY when disconnecting the ACTIVE backend (see the callers): that is the
 * only case where the backend re-arms its `AuthExpiredGuard` dedup latch (via
 * promotion/teardown), so the frontend flag and the backend latch stay in
 * lock-step. Clearing it on a non-active disconnect would dismiss the toast
 * while the latch stays closed, permanently suppressing a genuinely-expired
 * active session's prompt (review). Dynamic import avoids a static
 * store<->events cycle.
 */
async function clearAuthExpiredUi(): Promise<void> {
  jellyfinState.authExpired = false;
  try {
    const { dismissAuthExpiredToast } = await import('#lib/services/events.js');
    dismissAuthExpiredToast();
  } catch (err) {
    console.warn('Failed to dismiss auth-expired toast:', err);
  }
}

/**
 * Disconnect from the Jellyfin backend.
 *
 * Clears ONLY Jellyfin's credentials; if Jellyfin was active and OpenSubsonic is
 * still connected, the backend promotes OpenSubsonic. We therefore refresh
 * status (rather than hard-setting disconnected) so the store reflects whatever
 * backend — if any — remains live.
 */
export async function disconnectJellyfin(): Promise<void> {
  // Only the ACTIVE backend carries the auth-expired session, and only an
  // active-backend disconnect re-arms the backend's dedup latch (via
  // promotion/teardown). Clearing the expired UI on a NON-active disconnect
  // would dismiss a genuinely-expired active session's prompt while the backend
  // latch stays closed, permanently suppressing it (review).
  const wasActive = getActiveBackend() === 'jellyfin';
  try {
    await invoke('disconnect_jellyfin');
    jellyfinState.lastError = null;
    jellyfinState.quickConnectCode = null;
    jellyfinState.quickConnectSecret = null;
    await refreshJellyfinStatus();
    if (wasActive) await clearAuthExpiredUi();
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    jellyfinState.lastError = error;
    throw e;
  }
}

/**
 * Disconnect from the OpenSubsonic backend.
 *
 * Clears ONLY OpenSubsonic's credentials; if it was active and Jellyfin is still
 * connected, the backend promotes Jellyfin. Refresh status to reflect the result;
 * clear the session-expired UI only when OpenSubsonic was the active backend (see
 * clearAuthExpiredUi — that's when the backend re-arms its latch).
 */
export async function disconnectSubsonic(): Promise<void> {
  // See disconnectJellyfin: only clear the expired UI when the disconnected
  // backend was the active one (mirrors the backend re-arming its latch).
  const wasActive = getActiveBackend() === 'opensubsonic';
  try {
    await invoke('disconnect_subsonic');
    jellyfinState.lastError = null;
    await refreshJellyfinStatus();
    if (wasActive) await clearAuthExpiredUi();
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    jellyfinState.lastError = error;
    throw e;
  }
}

/**
 * Test connection to Jellyfin server.
 */
export async function testJellyfinConnection(): Promise<boolean> {
  try {
    await invoke('test_jellyfin_connection');
    return true;
  } catch (e) {
    console.error('Jellyfin connection test failed:', e);
    return false;
  }
}

/**
 * Refresh connection status from the backend.
 *
 * Fetches BOTH the active-backend status (drives the app-wide library/playback
 * gate) and the per-backend credential presence (drives the dual-backend
 * Connections UI). Mutates state properties directly per the repo rune rule.
 */
export async function refreshJellyfinStatus(): Promise<void> {
  try {
    const [status, backends] = await Promise.all([invoke<ConnectionStatus>('get_jellyfin_status'), invoke<MediaBackendsStatus>('get_media_backends')]);
    jellyfinState.status = status;
    jellyfinState.backends = {
      jellyfin: backends.jellyfin,
      opensubsonic: backends.opensubsonic,
    };
    connectionGeneration++;
  } catch (e) {
    console.error('Failed to get media status:', e);
  }
}

/**
 * Switch which connected backend is active.
 *
 * Swaps the live media client + proxy in the backend and flips the DB's active
 * source, then refreshes status so the library-reload effect re-reads the cache
 * for the new backend. NO re-sync — both libraries are already cached.
 */
export async function setActiveBackend(serverType: BackendKind): Promise<void> {
  await invoke('set_active_backend', { serverType });
  await refreshJellyfinStatus();
}

/**
 * Initialise Jellyfin state from backend.
 */
export async function initJellyfinState(): Promise<void> {
  await refreshJellyfinStatus();
}
