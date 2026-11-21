// Serves artwork from the LOCAL disk cache as the primary source via a Tauri
// custom protocol, falling back to the remote Jellyfin URL, then a placeholder.
//
// The custom protocol is registered under the scheme name "artwork" (see
// src-tauri/src/artwork.rs). The URL form the WebView expects is
// platform-dependent:
//   - Windows / Android:  http://artwork.localhost/{id}.jpg
//   - Linux / macOS / iOS: artwork://localhost/{id}.jpg
// Using the wrong form means the request never reaches the protocol handler
// and every cached image silently fails to load.

/** Resolve the platform-correct base URL for the custom artwork protocol. */
function detectArtworkBase(): string {
  const ua = typeof navigator !== 'undefined' ? navigator.userAgent.toLowerCase() : '';
  // Only Windows and Android use the `http://<scheme>.localhost` form; every
  // other platform (Linux, macOS, iOS) uses `<scheme>://localhost`. Default to
  // the latter for anything that isn't clearly Windows/Android.
  if (ua.includes('windows') || ua.includes('android')) {
    return 'http://artwork.localhost/';
  }
  return 'artwork://localhost/';
}

const ARTWORK_BASE = detectArtworkBase();

/** Reset fallback state when Svelte reuses an image for a different source. */
export function resetArtwork(img: HTMLImageElement, source: string | null | undefined) {
  const placeholder = img.nextElementSibling as HTMLElement | null;
  const placeholderDisplay = placeholder?.style.display ?? '';
  function reset(): void {
    delete img.dataset.artworkStage;
    img.style.display = '';
    if (placeholder) placeholder.style.display = placeholderDisplay;
  }
  return {
    update(next: string | null | undefined): void {
      if (next !== source) {
        source = next;
        reset();
      }
    },
  };
}

/** Build a locally-cached artwork URL (disk cache) from a Jellyfin item ID. */
export function cachedArtworkUrl(jellyfinId: string): string {
  return `${ARTWORK_BASE}${jellyfinId}.jpg`;
}

/**
 * Progressive image fallback handler.
 *
 * The <img> loads a primary `src` (normally the local cache URL). Each
 * failure advances exactly one stage:
 *   1. primary fails  -> swap to `fallbackUrl` (normally the Jellyfin URL)
 *   2. fallback fails -> hide the image and reveal the sibling placeholder
 *
 * A `data-artwork-stage` attribute records progress so a failing fallback
 * can't re-enter the same branch and loop forever. The stage names are
 * source-agnostic, so callers may use the cache as primary (cache-first) or
 * as the fallback (remote-first) without changing this logic.
 */
export function handleArtworkError(event: Event, fallbackUrl?: string | null): void {
  const img = event.currentTarget as HTMLImageElement;
  const stage = img.dataset.artworkStage ?? 'primary';

  // Primary failed and we have a distinct fallback to try — use it once.
  if (stage === 'primary' && fallbackUrl && img.src !== fallbackUrl) {
    img.dataset.artworkStage = 'fallback';
    img.src = fallbackUrl;
    return;
  }

  // Fallback failed (or there was nothing else to try) — show placeholder.
  img.dataset.artworkStage = 'failed';
  img.style.display = 'none';
  const placeholder = img.nextElementSibling as HTMLElement | null;
  if (placeholder) placeholder.style.display = '';
}
