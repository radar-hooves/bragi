// A transport command's rejection (play/pause/seek/skip) must never be
// swallowed inside the real app — that silence is what let a stuck backend
// look like "press play, nothing happens" with no toast, no log a user could
// find, nothing. Outside the Tauri shell (a browser-only dev preview,
// or these very tests) every invoke() rejects because there is no backend,
// and that rejection is expected and must stay silent.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const toastError = vi.fn();
vi.mock('svelte-sonner', () => ({
  toast: { error: (...args: unknown[]) => toastError(...args) },
}));

const { reportTransportError } = await import('./transportError');

describe('reportTransportError', () => {
  beforeEach(() => {
    toastError.mockClear();
    vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
  });

  it('stays silent outside the Tauri runtime (dev preview / tests)', () => {
    reportTransportError('Resume', new Error('no backend'));
    expect(toastError).not.toHaveBeenCalled();
  });

  it('surfaces a toast when running inside the real app', () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    reportTransportError('Resume', new Error('engine unreachable'));
    expect(toastError).toHaveBeenCalledTimes(1);
    const [title, opts] = toastError.mock.calls[0]!;
    expect(title).toBeTruthy();
    expect((opts as { description: string }).description).toContain('engine unreachable');
  });
});
