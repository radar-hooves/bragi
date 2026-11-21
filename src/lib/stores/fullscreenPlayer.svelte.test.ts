// Pins the in-app full-screen "Now Playing" player's open/close/toggle
// switching — the one piece of Bragi's window-state work that lives entirely
// on the frontend (transient view state, never persisted, matching how
// Apple Music/Spotify's own "Now Playing" full view resets on relaunch).
import { describe, it, expect } from 'vitest';

const { isFullScreenPlayerOpen, openFullScreenPlayer, closeFullScreenPlayer, toggleFullScreenPlayer } =
  await import('./fullscreenPlayer.svelte');

describe('fullscreenPlayer', () => {
  it('starts closed', () => {
    expect(isFullScreenPlayerOpen()).toBe(false);
  });

  it('opens and closes', () => {
    openFullScreenPlayer();
    expect(isFullScreenPlayerOpen()).toBe(true);
    closeFullScreenPlayer();
    expect(isFullScreenPlayerOpen()).toBe(false);
  });

  it('open is idempotent', () => {
    openFullScreenPlayer();
    openFullScreenPlayer();
    expect(isFullScreenPlayerOpen()).toBe(true);
    closeFullScreenPlayer();
  });

  it('close is idempotent', () => {
    closeFullScreenPlayer();
    closeFullScreenPlayer();
    expect(isFullScreenPlayerOpen()).toBe(false);
  });

  it('toggle flips whichever state it is currently in', () => {
    closeFullScreenPlayer();
    toggleFullScreenPlayer();
    expect(isFullScreenPlayerOpen()).toBe(true);
    toggleFullScreenPlayer();
    expect(isFullScreenPlayerOpen()).toBe(false);
  });
});
