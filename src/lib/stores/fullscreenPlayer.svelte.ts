/**
 * Full-screen "now playing" player visibility.
 *
 * A transient view flag (like the lyrics panel) — not persisted. The
 * full-screen player shows the album art, transport, and flowing lyrics over a
 * blurred backdrop, opened by clicking the now-playing artwork.
 */

let visible = $state(false);

/** Whether the full-screen player is open. */
export function isFullScreenPlayerOpen(): boolean {
  return visible;
}

/** Open the full-screen player. */
export function openFullScreenPlayer(): void {
  visible = true;
}

/** Close the full-screen player. */
export function closeFullScreenPlayer(): void {
  visible = false;
}

/** Toggle the full-screen player. */
export function toggleFullScreenPlayer(): void {
  visible = !visible;
}
