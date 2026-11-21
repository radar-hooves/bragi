/**
 * The one global keyboard-shortcut layer, mounted once in
 * `+layout.svelte`: Quit, MiniPlayer, column browser, reveal-in-library,
 * back/forward, play/pause, focus-search and UI zoom all live here, and every
 * one of them backs off a focused text field. Settings > Advanced > Keyboard shortcuts
 * documents Space (play/pause), ⌘/Ctrl+F (focus search) and the zoom trio
 * alongside the rest; this is what actually implements them.
 *
 * A pure, dependency-injected handler — no store imports — so it's testable
 * with a plain `KeyboardEvent` and spies, without mounting Svelte.
 */

/** True while the user is typing — a text input, a textarea, or a contenteditable region. Every shortcut below backs off here. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === 'TEXTAREA') return true;
  if (tag === 'INPUT') {
    // A checkbox/radio/range/button input has no text to protect — only the
    // text-entry input types should block a shortcut like Space.
    const type = (target as HTMLInputElement).type;
    return !['checkbox', 'radio', 'range', 'button', 'submit', 'reset', 'file', 'color'].includes(type);
  }
  // `contentEditable` (the reflected attribute) rather than the computed
  // `isContentEditable` — jsdom, this repo's test environment, never
  // implements the latter's inherited-from-ancestor resolution.
  return target.contentEditable === 'true';
}

export interface GlobalShortcutActions {
  togglePlayPause: () => void;
  quit: () => void;
  toggleMiniPlayer: () => void;
  toggleColumnBrowser: () => void;
  toggleUpNext: () => void;
  revealPlayingTrack: () => void;
  navigateBack: () => void;
  navigateForward: () => void;
  focusSearch: () => void;
  zoomIn: () => void;
  zoomOut: () => void;
  zoomReset: () => void;
}

/**
 * The whole main-window shortcut table. Returns true when it
 * handled the event (and already called `preventDefault`), so a caller
 * doesn't need to duplicate the guard logic.
 */
export function handleGlobalShortcut(event: KeyboardEvent, actions: GlobalShortcutActions): boolean {
  if (event.defaultPrevented || isTypingTarget(event.target)) return false;

  const mod = event.metaKey || event.ctrlKey;

  // Space: play/pause anywhere except a text field (Settings > Advanced > Keyboard
  // shortcuts has always documented this; nothing ever implemented it).
  if (event.code === 'Space' && !mod && !event.altKey) {
    event.preventDefault();
    actions.togglePlayPause();
    return true;
  }

  if (mod && event.key === 'q') {
    event.preventDefault();
    actions.quit();
    return true;
  }

  if (mod && event.shiftKey && event.key.toLowerCase() === 'm') {
    event.preventDefault();
    actions.toggleMiniPlayer();
    return true;
  }

  if (mod && event.key === 'b') {
    event.preventDefault();
    actions.toggleColumnBrowser();
    return true;
  }

  // ⌘/Ctrl+U — Up Next panel (Apple Music/Spotify both keep a queue toggle
  // near the transport controls; this is the app-wide equivalent shortcut).
  if (mod && event.key === 'u') {
    event.preventDefault();
    actions.toggleUpNext();
    return true;
  }

  // ⌘L — "Show in Library" for the now-playing track.
  if (mod && event.key === 'l') {
    event.preventDefault();
    actions.revealPlayingTrack();
    return true;
  }

  // ⌘F — focus the library search field (documented in Settings, never wired).
  if (mod && event.key === 'f') {
    event.preventDefault();
    actions.focusSearch();
    return true;
  }

  // Back/forward keep their original modifiers exactly (⌘/Alt, not Ctrl —
  // Ctrl+Arrow is left free for anything else that wants it).
  if ((event.metaKey || event.altKey) && event.key === 'ArrowLeft') {
    event.preventDefault();
    actions.navigateBack();
    return true;
  }

  if ((event.metaKey || event.altKey) && event.key === 'ArrowRight') {
    event.preventDefault();
    actions.navigateForward();
    return true;
  }

  // UI zoom — ⌘/Ctrl+Plus/Minus/0, the convention every Chromium/Electron
  // interface (Spotify's desktop app included) uses for whole-app zoom, since
  // the whole window is a webview rather than native AppKit/Win32 chrome.
  // '=' is the unshifted key that produces '+' on a US layout, so both are
  // matched without an explicit shiftKey check (Shift+= already reads as '+').
  if (mod && (event.key === '=' || event.key === '+')) {
    event.preventDefault();
    actions.zoomIn();
    return true;
  }

  if (mod && event.key === '-') {
    event.preventDefault();
    actions.zoomOut();
    return true;
  }

  if (mod && event.key === '0') {
    event.preventDefault();
    actions.zoomReset();
    return true;
  }

  return false;
}
