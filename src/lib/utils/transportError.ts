import { toast } from 'svelte-sonner';

/**
 * Whether this window is actually running inside the Tauri shell.
 *
 * The transport controls (play/pause/seek/skip/volume) are usable in a
 * browser-only dev preview (`vite dev` outside `tauri dev`) or under vitest,
 * where every `invoke()` call rejects because there is no backend — that
 * rejection is expected and must stay silent. Inside the real app, the exact
 * same rejection means a real command failure (the engine is unreachable, or
 * genuinely errored) and must never be swallowed: that silence is what let
 * "press play, nothing happens" go unnoticed.
 */
function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Report a transport-command failure (play/pause/seek/skip/volume) the same
 * way a backend-originated playback error is reported (`events.ts`'s
 * `playback-error` toast) — never silently swallowed inside the real app.
 */
export function reportTransportError(action: string, err: unknown): void {
  if (!isTauriRuntime()) return;
  console.error(`Transport command failed (${action}):`, err);
  toast.error('Playback command failed', {
    description: `${action} — ${err instanceof Error ? err.message : String(err)}`,
  });
}
