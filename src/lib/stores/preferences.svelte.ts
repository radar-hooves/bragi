// The one settings store for everyday preference toggles: notifications,
// startup/tray behaviour, playback (crossfade/gapless/ReplayGain/scrobbling),
// downloads, library sync/recently-added, and AI-playlist configuration
// (excluding the API key, which is a credential and stays local to AiPage).
// Loaded once at boot (loadPreferences(), called from the root layout);
// every setter applies immediately and a single generic effect accumulates
// whatever changed into a debounced batch write, retrying anything a failed
// write left unconfirmed, so a page never re-fetches on mount and never
// hand-rolls its own onMount/effect/loaded gate.
//
// Two settings carry a side effect beyond the DB (an OS call, a live
// audio-engine apply) and keep their own dedicated command instead of the
// generic batch: startupLaunchOnLogin and the fade pair. Layout/display/sort
// state (accent colour, zoom, view modes, column widths) is a separate
// concern owned by the sorting code and stays in `ui.svelte.ts`; playback
// transport state (shuffle/repeat) stays in `player.svelte.ts` (playback); the MiniPlayer's `miniPlayerAlwaysOnTop` stays local (window);
// connection credentials stay in `jellyfin.svelte.ts` / `scrobble.svelte.ts`.
import { toast } from 'svelte-sonner';
import type { AppSettings } from '#lib/types/tauri.js';
import { getSettings, updateSettings, setFadeSettings as setFadeSettingsCommand, setLaunchOnLogin as setLaunchOnLoginCommand } from '#lib/services/tauri.js';
import { setScrobbleSettings as mirrorScrobbleSettings } from './scrobble.svelte';

type GenericKey =
  | 'songNotificationsEnabled'
  | 'startMinimised'
  | 'closeToTray'
  | 'minimiseToTray'
  | 'playCountSource'
  | 'crossfadeDuration'
  | 'gaplessPlayback'
  | 'replayGainMode'
  | 'replayGainPreamp'
  | 'scrobbleEnabled'
  | 'scrobbleThreshold'
  | 'aiPlaylistEnabled'
  | 'llmProvider'
  | 'llmModel'
  | 'llmBaseUrl'
  | 'llmSystemPrompt'
  | 'llmMaxTokens'
  | 'downloadFormat'
  | 'downloadBitrate'
  | 'librarySyncInterval'
  | 'recentlyAddedDays';

/** Persisted through their own command (an OS side effect or a live apply),
 * never the generic batch below. */
type DedicatedKey = 'startupLaunchOnLogin' | 'fadeEnabled' | 'fadeDurationMs';

/** Server-computed, never written by us: whether a key is in the keyring for
 * the selected provider. Cached here purely so AiPage does not need its own
 * `getSettings()` round trip on mount; kept fresh by `setLlmHasApiKey` after a
 * save/clear/test. */
type ReadOnlyKey = 'llmHasApiKey';

export type Preferences = Pick<AppSettings, GenericKey | DedicatedKey | ReadOnlyKey>;

const DEFAULTS: Preferences = {
  songNotificationsEnabled: false,
  startMinimised: false,
  closeToTray: true,
  minimiseToTray: false,
  playCountSource: 'auto',
  crossfadeDuration: 0,
  gaplessPlayback: true,
  replayGainMode: 'track',
  replayGainPreamp: 0,
  scrobbleEnabled: true,
  scrobbleThreshold: 50,
  aiPlaylistEnabled: false,
  llmProvider: 'ollama',
  llmModel: '',
  llmBaseUrl: '',
  llmSystemPrompt: '',
  llmMaxTokens: 2048,
  downloadFormat: 'original',
  downloadBitrate: 256,
  librarySyncInterval: 0,
  recentlyAddedDays: 30,
  startupLaunchOnLogin: false,
  fadeEnabled: true,
  fadeDurationMs: 400,
  llmHasApiKey: false,
};

// Keys the generic diff-and-persist effect skips: two carry a side effect
// beyond the DB and go through their own command, one is a read-only cache
// never written by us.
const NON_GENERIC_KEYS = new Set<keyof Preferences>(['startupLaunchOnLogin', 'fadeEnabled', 'fadeDurationMs', 'llmHasApiKey']);

const prefs = $state<Preferences>({ ...DEFAULTS });
let loaded = $state(false);

// `lastPersisted` reflects only what the backend has confirmed — a key moves
// here only once its `updateSettings` call resolves. `pending` accumulates
// every key that still differs from that confirmed state, merged across
// effect runs rather than replaced, so a second field changing inside the
// debounce window adds to the batch instead of replacing it. A failed write
// leaves its keys in `pending`: nothing rearms the timer on its own, but the
// next change re-triggers the effect, which re-detects the still-unpersisted
// keys and folds them into that write.
let lastPersisted: Preferences = { ...DEFAULTS };
let pending: Partial<Record<keyof Preferences, unknown>> = {};
let flushing = false;
let saveTimeout: ReturnType<typeof setTimeout> | null = null;

function scheduleFlush(): void {
  if (saveTimeout) clearTimeout(saveTimeout);
  saveTimeout = setTimeout(() => {
    saveTimeout = null;
    void flushPending();
  }, 300);
}

async function flushPending(): Promise<void> {
  // A flush is already in flight — its own `finally` re-checks `pending` once
  // it resolves, so anything added meanwhile is not lost, just not raced.
  if (flushing) return;
  const toSend = { ...pending };
  const keys = Object.keys(toSend) as (keyof Preferences)[];
  if (keys.length === 0) return;

  flushing = true;
  let sent = false;
  try {
    await updateSettings(toSend as Partial<AppSettings>);
    sent = true;
    for (const key of keys) {
      (lastPersisted as Record<string, unknown>)[key] = toSend[key];
      // Only clear a key if it has not changed again since this send was
      // captured; if it has, it stays pending for the next flush.
      if (pending[key] === toSend[key]) delete pending[key];
    }
  } catch (err) {
    console.error('Failed to persist preferences:', err);
    toast.error("Couldn't save your settings", { description: 'They will save with your next change.' });
    // Leave every key in `toSend` in `pending` — the next change re-runs the
    // effect, which re-detects them (still != lastPersisted) and retries.
  } finally {
    flushing = false;
    // Only a successful send re-flushes what changed while it was in flight;
    // after a failure, rescheduling here would retry a refused write forever.
    if (sent && Object.keys(pending).length > 0) scheduleFlush();
  }
}

$effect.root(() => {
  $effect(() => {
    if (!loaded) return;
    const current = $state.snapshot(prefs);
    let hasNewDiff = false;
    for (const key of Object.keys(current) as (keyof Preferences)[]) {
      if (NON_GENERIC_KEYS.has(key)) continue;
      if (current[key] !== lastPersisted[key]) {
        pending[key] = current[key];
        hasNewDiff = true;
      }
    }
    if (hasNewDiff) scheduleFlush();
  });
});

/** The reactive preferences state. Read directly in markup (`getPreferences().x`) —
 * it is already loaded and current by the time any pane mounts. */
export function getPreferences(): Preferences {
  return prefs;
}

/** Fetch persisted preferences once. Call at app startup, alongside
 * `loadUIState()`. Idempotent-safe to call again (e.g. in a test), but not
 * needed per-pane-mount — that per-page re-fetch was the pane-switch cost
 * this store replaces. */
export async function loadPreferences(): Promise<void> {
  try {
    const settings = await getSettings();
    const next: Preferences = {
      songNotificationsEnabled: settings.songNotificationsEnabled,
      startMinimised: settings.startMinimised,
      closeToTray: settings.closeToTray,
      minimiseToTray: settings.minimiseToTray,
      playCountSource: settings.playCountSource,
      crossfadeDuration: settings.crossfadeDuration,
      gaplessPlayback: settings.gaplessPlayback,
      replayGainMode: settings.replayGainMode,
      replayGainPreamp: settings.replayGainPreamp,
      scrobbleEnabled: settings.scrobbleEnabled,
      scrobbleThreshold: settings.scrobbleThreshold,
      aiPlaylistEnabled: settings.aiPlaylistEnabled,
      llmProvider: settings.llmProvider,
      llmModel: settings.llmModel,
      llmBaseUrl: settings.llmBaseUrl,
      llmSystemPrompt: settings.llmSystemPrompt,
      llmMaxTokens: settings.llmMaxTokens,
      downloadFormat: settings.downloadFormat,
      downloadBitrate: settings.downloadBitrate,
      librarySyncInterval: settings.librarySyncInterval,
      recentlyAddedDays: settings.recentlyAddedDays,
      startupLaunchOnLogin: settings.startupLaunchOnLogin,
      fadeEnabled: settings.fadeEnabled,
      fadeDurationMs: settings.fadeDurationMs,
      llmHasApiKey: settings.llmHasApiKey,
    };
    lastPersisted = next;
    Object.assign(prefs, next);
    mirrorScrobbleSettings(next.scrobbleEnabled, next.scrobbleThreshold);
  } catch (err) {
    console.error('Failed to load preferences:', err);
  } finally {
    loaded = true;
  }
}

export function setSongNotificationsEnabled(value: boolean): void {
  prefs.songNotificationsEnabled = value;
}

export function setStartMinimised(value: boolean): void {
  prefs.startMinimised = value;
}

export function setCloseToTray(value: boolean): void {
  prefs.closeToTray = value;
}

export function setMinimiseToTray(value: boolean): void {
  prefs.minimiseToTray = value;
}

export function setPlayCountSource(value: Preferences['playCountSource']): void {
  prefs.playCountSource = value;
}

export function setCrossfadeDuration(ms: number): void {
  prefs.crossfadeDuration = ms;
}

export function setGaplessPlayback(value: boolean): void {
  prefs.gaplessPlayback = value;
}

export function setReplayGainMode(value: Preferences['replayGainMode']): void {
  prefs.replayGainMode = value;
}

export function setReplayGainPreamp(value: number): void {
  prefs.replayGainPreamp = value;
}

export function setScrobbleEnabled(value: boolean): void {
  prefs.scrobbleEnabled = value;
  mirrorScrobbleSettings(value, prefs.scrobbleThreshold);
}

export function setScrobbleThreshold(value: number): void {
  prefs.scrobbleThreshold = value;
  mirrorScrobbleSettings(prefs.scrobbleEnabled, value);
}

export function setAiPlaylistEnabled(value: boolean): void {
  prefs.aiPlaylistEnabled = value;
}

export function setLlmProvider(value: Preferences['llmProvider']): void {
  prefs.llmProvider = value;
}

export function setLlmModel(value: string): void {
  prefs.llmModel = value;
}

export function setLlmBaseUrl(value: string): void {
  prefs.llmBaseUrl = value;
}

export function setLlmSystemPrompt(value: string): void {
  prefs.llmSystemPrompt = value;
}

export function setLlmMaxTokens(value: number): void {
  prefs.llmMaxTokens = value;
}

export function setDownloadFormat(value: Preferences['downloadFormat']): void {
  prefs.downloadFormat = value;
}

export function setDownloadBitrate(value: number): void {
  prefs.downloadBitrate = value;
}

export function setLibrarySyncInterval(value: number): void {
  prefs.librarySyncInterval = value;
}

export function setRecentlyAddedDays(value: number): void {
  prefs.recentlyAddedDays = value;
}

/** Read-only cache, refreshed by AiPage after a key save/clear/test — never
 * sent to `updateSettings` (see `NON_GENERIC_KEYS`). */
export function setLlmHasApiKey(value: boolean): void {
  prefs.llmHasApiKey = value;
}

/** Launch-on-login performs the OS-level autostart enable/disable before
 * persisting, so it goes through its own command rather than the generic
 * batch above. Reverts and rethrows if the OS refuses, so the switch never
 * shows a state the system did not actually take. */
export async function setStartupLaunchOnLogin(value: boolean): Promise<void> {
  const previous = prefs.startupLaunchOnLogin;
  prefs.startupLaunchOnLogin = value;
  try {
    await setLaunchOnLoginCommand(value);
  } catch (err) {
    prefs.startupLaunchOnLogin = previous;
    throw err;
  }
}

/** Pause/stop fade applies live to the running audio engine, so it goes
 * through its own command rather than the generic batch above. Reverts both
 * fields and rethrows if the engine refuses. */
export async function setFadeSettings(enabled: boolean, durationMs: number): Promise<void> {
  const previous = { fadeEnabled: prefs.fadeEnabled, fadeDurationMs: prefs.fadeDurationMs };
  prefs.fadeEnabled = enabled;
  prefs.fadeDurationMs = durationMs;
  try {
    await setFadeSettingsCommand(enabled, durationMs);
  } catch (err) {
    prefs.fadeEnabled = previous.fadeEnabled;
    prefs.fadeDurationMs = previous.fadeDurationMs;
    throw err;
  }
}

/** Test-only: reset the module's in-memory state between tests. */
export function _resetPreferencesForTests(): void {
  Object.assign(prefs, DEFAULTS);
  lastPersisted = { ...DEFAULTS };
  pending = {};
  flushing = false;
  loaded = false;
  if (saveTimeout) {
    clearTimeout(saveTimeout);
    saveTimeout = null;
  }
}
