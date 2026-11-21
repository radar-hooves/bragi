// Sync/offline/downloads polish: getEffectiveOffline() collapses the user's manual toggle and
// the backend's automatic connectivity signal into one source of truth every
// "is this playable / should this be greyed out" decision reads.
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('#lib/services/tauri.js', () => ({
  getSettings: vi.fn().mockResolvedValue({ offlineMode: false }),
  updateSettings: vi.fn().mockResolvedValue(undefined),
}));

const {
  getOfflineMode,
  getNetworkReachable,
  getEffectiveOffline,
  setOfflineMode,
  setNetworkReachable,
} = await import('./offlineMode.svelte');

describe('getEffectiveOffline', () => {
  beforeEach(async () => {
    await setOfflineMode(false);
    setNetworkReachable(true);
  });

  it('is online when neither the toggle nor connectivity says otherwise', () => {
    expect(getEffectiveOffline()).toBe(false);
  });

  it('is offline when the user manually enables offline mode, network reachable or not', async () => {
    await setOfflineMode(true);
    expect(getOfflineMode()).toBe(true);
    expect(getEffectiveOffline()).toBe(true);

    setNetworkReachable(false);
    expect(getEffectiveOffline()).toBe(true);
  });

  it('is offline when connectivity is lost, even though the user never toggled the manual setting', () => {
    setNetworkReachable(false);
    expect(getOfflineMode()).toBe(false);
    expect(getNetworkReachable()).toBe(false);
    expect(getEffectiveOffline()).toBe(true);
  });

  it('returns online once connectivity is restored and the manual toggle is off', () => {
    setNetworkReachable(false);
    expect(getEffectiveOffline()).toBe(true);

    setNetworkReachable(true);
    expect(getEffectiveOffline()).toBe(false);
  });
});
