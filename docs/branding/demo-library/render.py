"""Render the README screenshots from the showcase library of real, freely licensed albums.

Runs the real front end (vite dev) in headless WebKit with a mocked Tauri IPC
backend that answers from showcase.py, serving each album's own cover from
docs/branding/showcase/art/. Output: docs/branding/screenshot-*.png

From the repo root, with node_modules installed:

  PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=1 nix shell --impure --expr \
    'with import (builtins.getFlake "nixpkgs") {}; python3.withPackages (p: [p.playwright])' \
    -c python3 docs/branding/demo-library/render.py

The window is captured at 1440x900 and 2x, then framed, inset 48px, in a
transparent 1536x996 canvas drawn at 1.2x: 1843 px wide, twice the README's 900. The invented library (library.py, art.py) stays the layout sweep's.
"""

import json
import os
import pathlib
import socket
import subprocess
import tempfile
import sys
import time

from playwright.sync_api import sync_playwright

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import showcase  # noqa: E402

ROOT = HERE.parents[2]
OUT = ROOT / "docs" / "branding"
MAC_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15"

WIN_W, WIN_H, MARGIN, DPR = 1440, 900, 48, 2
# The framed image is about twice the 900 px the README shows it at.
FRAME_DPR = 1.2

artists, albums, tracks = showcase.build()
by_album = {}
for t in tracks:
    by_album.setdefault(t["albumJellyfinId"], []).append(t)
album_by_name = {a["name"]: a for a in albums}

PLAYING = by_album[album_by_name["Last Refuge"]["jellyfinId"]][3]
COLUMN_ALBUM = album_by_name["The Shade From Our Trees"]


def clean(rows):
    return [{k: v for k, v in r.items() if k != "family"} for r in rows]


ALBUMS = clean(albums)
ARTISTS = clean(artists)
for a in ARTISTS:
    a.pop("genre", None)

# The commands the demo library answers, as window.__demoHandlers. Shared by the
# README renders (SHIM below) and the layout sweep (factory harness, layout-sweep.py).
HANDLERS = r"""
(() => {
  const D = window.__DEMO__;
  const settings = Object.assign({
    theme: 'dark', accentColour: 'blue', serverType: 'jellyfin', offlineMode: false,
    viewMode: 'grid', artistViewMode: 'grid', genreViewMode: 'grid', volume: 0.7,
    shuffle: false, repeatMode: 'off', sidebarCollapsed: false, queueSidebarVisible: false,
    statusBarVisible: true, columnBrowserVisible: false, uiZoomLevel: 100, recentlyAddedDays: 30,
    albumsSort: 'artist-asc', artistsSort: 'name-asc', tracksSort: 'name-asc',
    scrobbleEnabled: false, lastLibrarySync: 1760000000, librarySyncInterval: 24,
    gridTileSizes: {}, trackListViewColumns: {}, playlistTrackColumns: {},
  }, D.settings);
  const status = { connected: true, serverUrl: 'https://music.example.net', serverType: 'jellyfin' };

  function view(p) {
    p = p || {};
    let ts = D.tracks.slice();
    if (p.genres && p.genres.length) ts = ts.filter(t => p.genres.includes(t.genre));
    if (p.artists && p.artists.length) ts = ts.filter(t => p.artists.includes(t.artistName));
    if (p.albumIds && p.albumIds.length) ts = ts.filter(t => p.albumIds.includes(t.albumJellyfinId));
    const byName = (a, b) => a.name.localeCompare(b.name);
    ts.sort(byName);
    const names = [...new Set(D.tracks.map(t => t.artistName))].sort((a, b) => a.localeCompare(b));
    const albs = D.albums
      .filter(a => !(p.artists && p.artists.length) || p.artists.includes(a.artistName))
      .map(a => ({ id: a.jellyfinId, name: a.name, artistName: a.artistName }))
      .sort(byName);
    return { columnBrowser: { genres: [...new Set(D.tracks.map(t => t.genre))].sort(), artists: names, albums: albs, totalTrackCount: ts.length }, tracks: ts };
  }

  window.__demoHandlers = {
    get_settings: () => settings,
    update_settings: () => null,
    get_jellyfin_status: () => status,
    get_media_backends: () => ({ active: 'jellyfin', jellyfin: { connected: true, serverUrl: status.serverUrl }, opensubsonic: { connected: false } }),
    get_cached_albums: () => D.albums, get_library_albums: () => D.albums,
    get_cached_artists: () => D.artists, get_library_artists: () => D.artists,
    get_all_cached_tracks: () => D.tracks,
    get_album_tracks: a => D.tracks.filter(t => t.albumJellyfinId === a.albumId),
    get_cached_tracks: a => D.tracks.filter(t => t.albumJellyfinId === a.albumId),
    get_artist_albums: a => D.albums.filter(x => x.artistJellyfinId === a.artistId),
    get_cached_artist_albums: a => D.albums.filter(x => x.artistJellyfinId === a.artistId),
    get_recently_added_albums: () => [],
    sync_library: () => null,
    query_library_view: a => view(a.params),
    query_tracks: a => view(a.params).tracks,
    get_genre_stats: () => [],
    get_jellyfin_playlists: () => D.playlists,
    get_queue: () => ({ tracks: [], currentIndex: 0, sources: [] }),
    get_playback_state: () => ({ currentTrack: null, isPlaying: false, currentTimeMs: 0, durationMs: 0, volume: 0.7, shuffle: false, repeat: 'off' }),
    get_download_queue: () => [], get_download_state: () => 'idle', get_downloaded_tracks: () => [],
    get_pending_sync_operations: () => [], get_output_devices: () => [],
    get_cache_settings: () => ({ maxSizeBytes: 10737418240 }),
    get_cache_stats: () => ({ totalSizeBytes: 0, trackCount: 0, maxSizeBytes: 10737418240 }),
    get_sync_status: () => ({ isConfigured: false, isOnline: false, pendingCount: 0 }),
    get_music_service_status: () => ({ connected: false }),
    get_server_play_counts: () => ({}), refresh_track_stats: () => ({}),
    get_imported_playlists: () => [],
    get_eq_status: () => ({ enabled: false, preset: 'Flat', bands: [] }),
  };
})();
"""

SHIM = r"""
(() => {
  const cbs = {}; let cbId = 1;
  window.__listeners = {}; let lid = 1;
  const unknown = new Set(); window.__unknown = unknown;
  const handlers = Object.assign({}, window.__demoHandlers, {
    'plugin:event|listen': a => { const id = lid++; (window.__listeners[a.event] = window.__listeners[a.event] || []).push(a.handler); return id; },
    'plugin:event|unlisten': () => null,
  });

  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    transformCallback: (cb) => { const id = cbId++; cbs[id] = cb; return id; },
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => {
      const h = handlers[cmd];
      if (h) return h(args || {});
      unknown.add(cmd);
      return null;
    },
  };
  window.__fire = (event, payload) => {
    for (const id of (window.__listeners[event] || [])) cbs[id]({ event, id: 0, payload });
  };
})();
"""

MOCK = HANDLERS + SHIM

# The app reads its artwork cache through the `artwork://` protocol Tauri serves;
# headless WebKit has none, so cache URLs go to the showcase's own covers.
ARTWORK_CACHE = r"""
(() => {
  const fix = v => typeof v === 'string' && v.startsWith('artwork://localhost/') ? 'http://demo.art/' + v.slice(20) : v;
  const d = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src');
  Object.defineProperty(HTMLImageElement.prototype, 'src', { get() { return d.get.call(this); }, set(v) { d.set.call(this, fix(v)); }, configurable: true });
  const sa = Element.prototype.setAttribute;
  Element.prototype.setAttribute = function (n, v) { return sa.call(this, n, n === 'src' ? fix(v) : v); };
})();
"""

TRAFFIC = "".join(
    f'<i style="position:absolute;left:{x}px;top:20px;width:13px;height:13px;border-radius:50%;background:{c}"></i>'
    for x, c in ((13, "#ff5f57"), (36, "#febc2e"), (58, "#28c840"))
)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def frame(browser, shot_png, dest):
    with tempfile.TemporaryDirectory() as tmp:
        (pathlib.Path(tmp) / "shot.png").write_bytes(shot_png)
        html = (
            f'<body style="margin:0;background:transparent"><div style="position:absolute;left:{MARGIN}px;top:{MARGIN}px;width:{WIN_W}px;height:{WIN_H}px;'
            f'border-radius:12px;overflow:hidden;box-shadow:0 0 0 1px rgba(255,255,255,.12),0 22px 60px rgba(0,0,0,.45),0 4px 14px rgba(0,0,0,.35)">'
            f'<img src="shot.png" width="{WIN_W}" height="{WIN_H}" style="display:block">{TRAFFIC}</div></body>'
        )
        (pathlib.Path(tmp) / "frame.html").write_text(html)
        page = browser.new_page(
            viewport={"width": WIN_W + 2 * MARGIN, "height": WIN_H + 2 * MARGIN},
            device_scale_factor=FRAME_DPR,
        )
        page.goto((pathlib.Path(tmp) / "frame.html").as_uri())
        page.evaluate("document.images[0].decode()")
        page.wait_for_timeout(400)
        page.screenshot(path=str(dest), omit_background=True)
        page.close()


def shoot(browser, base, name, section, settings, after=None):
    data = {
        "albums": ALBUMS,
        "artists": ARTISTS,
        "tracks": tracks,
        "playlists": [
            {"jellyfinId": f"pl{i}", "name": n, "itemCount": c}
            for i, (n, c) in enumerate(showcase.PLAYLISTS)
        ],
        "settings": settings,
    }
    ctx = browser.new_context(
        viewport={"width": WIN_W, "height": WIN_H},
        device_scale_factor=DPR,
        user_agent=MAC_UA,
    )
    ctx.add_init_script(f"window.__DEMO__ = {json.dumps(data)};")
    ctx.add_init_script(MOCK)
    ctx.add_init_script(ARTWORK_CACHE)

    def serve(route):
        cover = showcase.SHOWCASE / "art" / route.request.url.rsplit("/", 1)[1]
        if not cover.is_file():
            return route.fulfill(status=404)
        route.fulfill(
            status=200,
            content_type="image/jpeg",
            body=cover.read_bytes(),
            headers={"Access-Control-Allow-Origin": "*"},
        )

    ctx.route("http://demo.art/**", serve)
    page = ctx.new_page()
    page.on("pageerror", lambda e: print("pageerror:", e))
    page.on("console", lambda m: m.type == "error" and print("console:", m.text[:300]))
    page.goto(base)
    page.wait_for_function(
        "window.__listeners && window.__listeners['playback-state-changed']"
    )
    page.evaluate(
        """(t) => {
          window.__fire('queue-changed', { type: 'queueChanged', tracks: [t], currentIndex: 0, sources: ['context'] });
          window.__fire('playback-state-changed', { type: 'stateChanged', currentTrack: t, isPlaying: true,
            currentTimeMs: 72000, durationMs: t.durationMs, volume: 0.7, shuffle: false, repeat: 'off', outputDevice: null });
        }""",
        PLAYING,
    )
    page.get_by_role("button", name=section, exact=True).first.click()
    if after:
        after(page)
    page.wait_for_timeout(1500)
    page.evaluate("document.fonts.ready")
    png = page.screenshot()
    print(name, "unknown commands:", page.evaluate("[...window.__unknown]"))
    ctx.close()
    if os.environ.get("DEMO_RAW"):
        pathlib.Path(os.environ["DEMO_RAW"], f"{name}.png").write_bytes(png)
    frame(browser, png, OUT / f"screenshot-{name}.png")


def main():
    port = free_port()
    vite = subprocess.Popen(
        [
            str(ROOT / "node_modules/.bin/vite"),
            "dev",
            "--port",
            str(port),
            "--strictPort",
        ],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        base = f"http://localhost:{port}/"
        for _ in range(120):
            try:
                socket.create_connection(("127.0.0.1", port), 1).close()
                break
            except OSError:
                time.sleep(0.5)
        with sync_playwright() as p:
            browser = p.webkit.launch()
            shoot(browser, base, "albums-grid", "Albums", {})


            def pick(page):
                page.get_by_text(COLUMN_ALBUM["artistName"], exact=True).first.click()
                page.wait_for_timeout(500)
                page.get_by_text(COLUMN_ALBUM["name"], exact=True).first.click()

            shoot(
                browser,
                base,
                "column-browser",
                "Songs",
                {
                    "columnBrowserVisible": True,
                    "columnBrowserColumns": ["artists", "albums"],
                    "columnBrowserHeight": 330,
                },
                pick,
            )
            browser.close()
    finally:
        vite.terminate()
        vite.wait()


if __name__ == "__main__":
    main()
