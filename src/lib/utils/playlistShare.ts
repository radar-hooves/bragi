import type { ImportPlaylistTrackInput, JellyfinPlaylistTrack } from '#lib/types/models.js';

/**
 * Portable, cross-instance playlist payload.
 *
 * Deliberately self-contained: no Jellyfin IDs, since the importing instance
 * has its own, unrelated library. `mbid` is the primary resolution key on
 * import; the rest is human-readable/fuzzy-match fallback for tracks a
 * library never tagged with a MusicBrainz recording ID.
 */
interface PlaylistSharePayload {
  name: string;
  tracks: ImportPlaylistTrackInput[];
}

const CURRENT_PAYLOAD_VERSION = 1;

/** Build the portable payload from a playlist's tracks. */
export function buildPlaylistSharePayload(
  name: string,
  tracks: JellyfinPlaylistTrack[]
): PlaylistSharePayload {
  return {
    name,
    tracks: tracks.map((t) => ({
      mbid: t.mbid,
      title: t.name,
      artistName: t.artistName,
      albumName: t.albumName,
      durationMs: t.durationMs,
    })),
  };
}

/**
 * Encode a playlist share payload as a compact, copy-pasteable code.
 *
 * Plain base64url JSON — no compression. Playlists are small enough (a few KB
 * of JSON even at hundreds of tracks) that adding a compression dependency
 * isn't worth it for v1; the code is meant for paste-into-a-dialog, not a
 * length-constrained URL bar.
 */
export function encodePlaylistShareCode(name: string, tracks: JellyfinPlaylistTrack[]): string {
  const payload = { v: CURRENT_PAYLOAD_VERSION, ...buildPlaylistSharePayload(name, tracks) };
  const json = JSON.stringify(payload);
  const bytes = new TextEncoder().encode(json);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export class PlaylistShareCodeError extends Error {}

/** Decode a share code back into a playlist payload. Throws on malformed input. */
export function decodePlaylistShareCode(code: string): PlaylistSharePayload {
  const trimmed = code.trim();
  if (!trimmed) {
    throw new PlaylistShareCodeError('Paste a playlist share code first.');
  }

  let json: string;
  try {
    const base64 = trimmed.replace(/-/g, '+').replace(/_/g, '/');
    const padded = base64.padEnd(base64.length + ((4 - (base64.length % 4)) % 4), '=');
    const binary = atob(padded);
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    json = new TextDecoder().decode(bytes);
  } catch {
    throw new PlaylistShareCodeError("That doesn't look like a valid playlist share code.");
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    throw new PlaylistShareCodeError("That doesn't look like a valid playlist share code.");
  }

  if (
    typeof parsed !== 'object' ||
    parsed === null ||
    !('name' in parsed) ||
    !('tracks' in parsed) ||
    typeof (parsed as { name: unknown }).name !== 'string' ||
    !Array.isArray((parsed as { tracks: unknown }).tracks)
  ) {
    throw new PlaylistShareCodeError('This playlist code is missing its name or track list.');
  }

  const { name, tracks } = parsed as PlaylistSharePayload;
  if (tracks.length === 0) {
    throw new PlaylistShareCodeError('This playlist code has no tracks.');
  }

  return { name, tracks };
}

export interface JellyfinItemLink {
  serverUrl: string;
  itemId: string;
}

/**
 * Parse a Jellyfin web-client "copy link" for an item, e.g.
 * `https://jellyfin.example.com/web/#/details?id=<itemId>&serverId=<serverId>`.
 *
 * Jellyfin's web client is a hash router, so the query string lives after the
 * `#` fragment rather than in the URL's normal search params — this is NOT a
 * Bragi share code, just a deep link that requires an authenticated session
 * on that specific server to resolve into anything.
 */
export function parseJellyfinItemLink(input: string): JellyfinItemLink | null {
  let url: URL;
  try {
    url = new URL(input.trim());
  } catch {
    return null;
  }

  const hashQueryIndex = url.hash.indexOf('?');
  const query = hashQueryIndex >= 0 ? url.hash.slice(hashQueryIndex + 1) : url.search.slice(1);
  const itemId = new URLSearchParams(query).get('id');
  if (!itemId) return null;

  return { serverUrl: url.origin, itemId };
}
