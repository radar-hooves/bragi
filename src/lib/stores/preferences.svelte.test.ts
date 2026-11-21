// Pins the settings-pane preference store's contract: documented defaults
// before load, an immediate apply on every setter (no "save" step), and a
// debounced write that accumulates every field changed inside the window
// (not just the last one) and retries anything a failed write left
// unconfirmed — the mechanism that replaced GeneralPage/PlaybackPage/AiPage's
// per-page onMount+effect+"loaded" boilerplate and AiPage's old Save button.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { flushSync } from 'svelte';
// The same file src-tauri/src/commands/settings.rs's
// `defaults_match_the_shared_frontend_fixture` test reads — one JSON file
// pins both languages' defaults against each other.
import SHARED_DEFAULTS_FIXTURE from '../../../src-tauri/fixtures/settings-defaults.json';

const getSettings = vi.fn();
const updateSettings = vi.fn().mockResolvedValue(undefined);
const setFadeSettingsCommand = vi.fn().mockResolvedValue(undefined);
const setLaunchOnLoginCommand = vi.fn().mockResolvedValue(undefined);

vi.mock('#lib/services/tauri.js', () => ({
  getSettings: () => getSettings(),
  updateSettings: (settings: unknown) => updateSettings(settings),
  setFadeSettings: (enabled: boolean, durationMs: number) => setFadeSettingsCommand(enabled, durationMs),
  setLaunchOnLogin: (enabled: boolean) => setLaunchOnLoginCommand(enabled),
}));

const mirrorScrobbleSettings = vi.fn();
vi.mock('./scrobble.svelte', () => ({
  setScrobbleSettings: (enabled: boolean, threshold: number) => mirrorScrobbleSettings(enabled, threshold),
}));

const toastError = vi.fn();
vi.mock('svelte-sonner', () => ({
  toast: { error: (...args: unknown[]) => toastError(...args) },
}));

const {
  getPreferences,
  loadPreferences,
  setGaplessPlayback,
  setReplayGainPreamp,
  setScrobbleEnabled,
  setScrobbleThreshold,
  setSongNotificationsEnabled,
  setCrossfadeDuration,
  setStartupLaunchOnLogin,
  setFadeSettings,
  setLlmHasApiKey,
  _resetPreferencesForTests,
} = await import('./preferences.svelte');

const LOADED_FIXTURE = {
  songNotificationsEnabled: true,
  startMinimised: true,
  closeToTray: false,
  minimiseToTray: true,
  playCountSource: 'earworm' as const,
  crossfadeDuration: 5000,
  gaplessPlayback: false,
  replayGainMode: 'album' as const,
  replayGainPreamp: 3,
  scrobbleEnabled: false,
  scrobbleThreshold: 80,
  aiPlaylistEnabled: true,
  llmProvider: 'openai' as const,
  llmModel: 'gpt-x',
  llmBaseUrl: 'https://example.com',
  llmSystemPrompt: 'custom prompt',
  llmMaxTokens: 4096,
  downloadFormat: 'flac' as const,
  downloadBitrate: 320,
  librarySyncInterval: 3600,
  recentlyAddedDays: 14,
  startupLaunchOnLogin: true,
  fadeEnabled: false,
  fadeDurationMs: 900,
  llmHasApiKey: true,
};

async function settle(): Promise<void> {
  flushSync();
  await vi.advanceTimersByTimeAsync(300);
}

describe('preferences store', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    _resetPreferencesForTests();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('matches the shared defaults fixture the Rust side also pins, before loadPreferences() resolves', () => {
    const prefs = getPreferences();
    for (const [key, expected] of Object.entries(SHARED_DEFAULTS_FIXTURE)) {
      expect(prefs[key as keyof typeof prefs]).toEqual(expected);
    }
  });

  it('loads persisted values from getSettings() and mirrors scrobble state for the audio engine', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);

    await loadPreferences();

    const prefs = getPreferences();
    expect(prefs.aiPlaylistEnabled).toBe(true);
    expect(prefs.llmProvider).toBe('openai');
    expect(prefs.crossfadeDuration).toBe(5000);
    expect(prefs.downloadFormat).toBe('flac');
    expect(prefs.librarySyncInterval).toBe(3600);
    expect(prefs.recentlyAddedDays).toBe(14);
    expect(prefs.llmHasApiKey).toBe(true);
    expect(mirrorScrobbleSettings).toHaveBeenCalledWith(false, 80);
  });

  it('does not write back the values it just loaded', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);

    await loadPreferences();
    await settle();

    expect(updateSettings).not.toHaveBeenCalled();
  });

  it('applies a setter immediately, ahead of any persistence', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();

    setGaplessPlayback(true);
    flushSync();

    expect(getPreferences().gaplessPlayback).toBe(true);
    expect(updateSettings).not.toHaveBeenCalled(); // still inside the debounce window
  });

  it('persists only the field that changed, with no save step', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();

    setReplayGainPreamp(7);
    await settle();

    expect(updateSettings).toHaveBeenCalledTimes(1);
    expect(updateSettings).toHaveBeenCalledWith({ replayGainPreamp: 7 });
  });

  it('coalesces rapid changes to the SAME field into a single debounced write of the final value', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();

    setScrobbleThreshold(60);
    flushSync();
    setScrobbleThreshold(65);
    flushSync();
    setScrobbleThreshold(70);
    await settle();

    expect(updateSettings).toHaveBeenCalledTimes(1);
    expect(updateSettings).toHaveBeenCalledWith({ scrobbleThreshold: 70 });
  });

  it('persists two DIFFERENT fields changed inside the same debounce window, not just the last one', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();

    setSongNotificationsEnabled(false);
    flushSync();
    setCrossfadeDuration(9000);
    await settle();

    expect(updateSettings).toHaveBeenCalledTimes(1);
    expect(updateSettings).toHaveBeenCalledWith({
      songNotificationsEnabled: false,
      crossfadeDuration: 9000,
    });
  });

  it('keeps a failed write pending and retries it together with the next change', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();
    updateSettings.mockRejectedValueOnce(new Error('offline'));

    setSongNotificationsEnabled(false);
    await settle(); // the write fails; the field stays pending, not lost

    expect(updateSettings).toHaveBeenCalledTimes(1);
    expect(getPreferences().songNotificationsEnabled).toBe(false); // still applied locally

    setCrossfadeDuration(9000); // any later change re-triggers a retry
    await settle();

    expect(updateSettings).toHaveBeenCalledTimes(2);
    expect(updateSettings).toHaveBeenLastCalledWith({
      songNotificationsEnabled: false,
      crossfadeDuration: 9000,
    });
  });

  it('never retries a refused write on its own, and tells the user once', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();
    updateSettings.mockRejectedValueOnce(new Error('refused'));

    setSongNotificationsEnabled(false);
    await settle();
    await vi.advanceTimersByTimeAsync(5000); // a self-rescheduling retry would fire here

    expect(updateSettings).toHaveBeenCalledTimes(1);
    expect(toastError).toHaveBeenCalledTimes(1);
  });

  it("mirrors every scrobble change into the audio engine's in-memory copy immediately", async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();
    mirrorScrobbleSettings.mockClear();

    setScrobbleEnabled(true);
    flushSync();

    expect(mirrorScrobbleSettings).toHaveBeenCalledWith(true, 80);
  });

  it('routes startup-launch-on-login and the fade pair through their own command, never the generic batch', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();

    await setStartupLaunchOnLogin(false);
    await setFadeSettings(true, 500);
    await settle();

    expect(setLaunchOnLoginCommand).toHaveBeenCalledWith(false);
    expect(setFadeSettingsCommand).toHaveBeenCalledWith(true, 500);
    expect(getPreferences().startupLaunchOnLogin).toBe(false);
    expect(getPreferences().fadeEnabled).toBe(true);
    expect(getPreferences().fadeDurationMs).toBe(500);
    expect(updateSettings).not.toHaveBeenCalled();
  });

  it('reverts startupLaunchOnLogin and rethrows when the OS refuses the autostart change', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE); // startupLaunchOnLogin: true
    await loadPreferences();
    setLaunchOnLoginCommand.mockRejectedValueOnce(new Error('permission denied'));

    await expect(setStartupLaunchOnLogin(false)).rejects.toThrow('permission denied');

    expect(getPreferences().startupLaunchOnLogin).toBe(true); // reverted, not left at the refused value
  });

  it('reverts the fade pair and rethrows when the audio engine refuses', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE); // fadeEnabled: false, fadeDurationMs: 900
    await loadPreferences();
    setFadeSettingsCommand.mockRejectedValueOnce(new Error('engine busy'));

    await expect(setFadeSettings(true, 500)).rejects.toThrow('engine busy');

    expect(getPreferences().fadeEnabled).toBe(false);
    expect(getPreferences().fadeDurationMs).toBe(900);
  });

  it('never sends the read-only API-key cache through the generic batch', async () => {
    getSettings.mockResolvedValue(LOADED_FIXTURE);
    await loadPreferences();

    setLlmHasApiKey(false);
    await settle();

    expect(getPreferences().llmHasApiKey).toBe(false);
    expect(updateSettings).not.toHaveBeenCalled();
  });
});
