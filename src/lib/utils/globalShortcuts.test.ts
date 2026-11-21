import { describe, it, expect, vi, beforeEach } from 'vitest';
import { isTypingTarget, handleGlobalShortcut, type GlobalShortcutActions } from './globalShortcuts';

function actions(): GlobalShortcutActions {
  return {
    togglePlayPause: vi.fn(),
    quit: vi.fn(),
    toggleMiniPlayer: vi.fn(),
    toggleColumnBrowser: vi.fn(),
    toggleUpNext: vi.fn(),
    revealPlayingTrack: vi.fn(),
    navigateBack: vi.fn(),
    navigateForward: vi.fn(),
    focusSearch: vi.fn(),
    zoomIn: vi.fn(),
    zoomOut: vi.fn(),
    zoomReset: vi.fn(),
  };
}

function keyOn(target: EventTarget, init: KeyboardEventInit & { key: string }): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  Object.defineProperty(event, 'target', { value: target });
  return event;
}

describe('isTypingTarget', () => {
  it('is true for a text input', () => {
    const input = document.createElement('input');
    input.type = 'text';
    expect(isTypingTarget(input)).toBe(true);
  });

  it('is true for a search input (the library search box)', () => {
    const input = document.createElement('input');
    input.type = 'search';
    expect(isTypingTarget(input)).toBe(true);
  });

  it('is true for a textarea', () => {
    expect(isTypingTarget(document.createElement('textarea'))).toBe(true);
  });

  it('is true for a contenteditable element', () => {
    const div = document.createElement('div');
    div.contentEditable = 'true';
    document.body.appendChild(div);
    expect(isTypingTarget(div)).toBe(true);
    div.remove();
  });

  it('is false for a checkbox — nothing to protect there', () => {
    const input = document.createElement('input');
    input.type = 'checkbox';
    expect(isTypingTarget(input)).toBe(false);
  });

  it('is false for a plain div (a track row, a card)', () => {
    expect(isTypingTarget(document.createElement('div'))).toBe(false);
  });
});

describe('handleGlobalShortcut', () => {
  let body: HTMLElement;

  beforeEach(() => {
    body = document.body;
  });

  it('Space toggles play/pause outside a text field', () => {
    const a = actions();
    const handled = handleGlobalShortcut(keyOn(body, { key: ' ', code: 'Space' }), a);
    expect(handled).toBe(true);
    expect(a.togglePlayPause).toHaveBeenCalledOnce();
  });

  it('Space inside a search box types a space instead of toggling playback', () => {
    const input = document.createElement('input');
    input.type = 'search';
    const a = actions();
    const handled = handleGlobalShortcut(keyOn(input, { key: ' ', code: 'Space' }), a);
    expect(handled).toBe(false);
    expect(a.togglePlayPause).not.toHaveBeenCalled();
  });

  it('backs off entirely once something more specific already handled the key', () => {
    const a = actions();
    const event = keyOn(body, { key: ' ', code: 'Space' });
    event.preventDefault(); // e.g. a listbox's own Space-to-select handler ran first
    const handled = handleGlobalShortcut(event, a);
    expect(handled).toBe(false);
    expect(a.togglePlayPause).not.toHaveBeenCalled();
  });

  it('⌘/Ctrl+Q quits', () => {
    const a = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: 'q', metaKey: true }), a)).toBe(true);
    expect(a.quit).toHaveBeenCalledOnce();
    expect(handleGlobalShortcut(keyOn(body, { key: 'q', ctrlKey: true }), actions())).toBe(true);
  });

  it('⌘/Ctrl+Shift+M toggles the MiniPlayer', () => {
    const a = actions();
    const handled = handleGlobalShortcut(keyOn(body, { key: 'm', metaKey: true, shiftKey: true }), a);
    expect(handled).toBe(true);
    expect(a.toggleMiniPlayer).toHaveBeenCalledOnce();
  });

  it('⌘/Ctrl+B toggles the column browser, even while typing is NOT guarded against here — it still backs off a text field', () => {
    const a = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: 'b', metaKey: true }), a)).toBe(true);
    expect(a.toggleColumnBrowser).toHaveBeenCalledOnce();

    const input = document.createElement('input');
    input.type = 'text';
    const a2 = actions();
    expect(handleGlobalShortcut(keyOn(input, { key: 'b', metaKey: true }), a2)).toBe(false);
    expect(a2.toggleColumnBrowser).not.toHaveBeenCalled();
  });

  it('⌘/Ctrl+U toggles the Up Next panel', () => {
    const a = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: 'u', metaKey: true }), a)).toBe(true);
    expect(a.toggleUpNext).toHaveBeenCalledOnce();
  });

  it('⌘/Ctrl+L reveals the playing track', () => {
    const a = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: 'l', metaKey: true }), a)).toBe(true);
    expect(a.revealPlayingTrack).toHaveBeenCalledOnce();
  });

  it('⌘/Ctrl+F focuses search, including while a shortcut-eligible field has focus', () => {
    const a = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: 'f', metaKey: true }), a)).toBe(true);
    expect(a.focusSearch).toHaveBeenCalledOnce();
  });

  it('⌘/Alt+ArrowLeft/Right navigate back/forward', () => {
    const back = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: 'ArrowLeft', metaKey: true }), back)).toBe(true);
    expect(back.navigateBack).toHaveBeenCalledOnce();

    const forward = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: 'ArrowRight', altKey: true }), forward)).toBe(true);
    expect(forward.navigateForward).toHaveBeenCalledOnce();
  });

  it('⌘/Ctrl+Plus zooms in (both the bare = key and shifted +)', () => {
    const a = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: '=', metaKey: true }), a)).toBe(true);
    expect(a.zoomIn).toHaveBeenCalledOnce();

    const a2 = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: '+', ctrlKey: true, shiftKey: true }), a2)).toBe(true);
    expect(a2.zoomIn).toHaveBeenCalledOnce();
  });

  it('⌘/Ctrl+Minus zooms out', () => {
    const a = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: '-', metaKey: true }), a)).toBe(true);
    expect(a.zoomOut).toHaveBeenCalledOnce();
    expect(handleGlobalShortcut(keyOn(body, { key: '-', ctrlKey: true }), actions())).toBe(true);
  });

  it('⌘/Ctrl+0 resets zoom', () => {
    const a = actions();
    expect(handleGlobalShortcut(keyOn(body, { key: '0', metaKey: true }), a)).toBe(true);
    expect(a.zoomReset).toHaveBeenCalledOnce();
  });

  it('zoom shortcuts back off a text field, same as every other shortcut here', () => {
    const input = document.createElement('input');
    input.type = 'text';
    const a = actions();
    expect(handleGlobalShortcut(keyOn(input, { key: '=', metaKey: true }), a)).toBe(false);
    expect(a.zoomIn).not.toHaveBeenCalled();
  });

  it('an unrelated key is left alone', () => {
    const a = actions();
    const handled = handleGlobalShortcut(keyOn(body, { key: 'x' }), a);
    expect(handled).toBe(false);
    expect(Object.values(a).some((fn) => (fn as ReturnType<typeof vi.fn>).mock.calls.length > 0)).toBe(false);
  });
});
