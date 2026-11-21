"""Bragi's app file for the factory layout sweep (scripts/webkit-layout/sweep.py).

Every view at every window size and UI zoom, in headless WebKit over the demo
library plus stress rows (stress.py: very long, very short, unbroken and art-less
names). From the repo root, with node_modules installed:

  nix shell --impure --expr \
    'with import (builtins.getFlake "nixpkgs") {}; python3.withPackages (p: [p.playwright])' \
    --command env PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=1 \
    python3 scripts/webkit-layout/sweep.py --app scripts/layout-sweep.py \
    --serve "pnpm exec vite dev --port {port} --strictPort"

scripts/webkit-layout/ is the factory harness vendored at its release tag
(app-factory v2026.10.38): replace it from skeletons/desktop/scripts/webkit-layout/
at the new tag, never edit it here. The `mini` variant is the mini-player window
and ignores the swept window size: each of its views sets its own.
"""

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
DEMO = ROOT / "docs" / "branding" / "demo-library"
sys.path.insert(0, str(DEMO))
import art  # noqa: E402
import library  # noqa: E402
import render  # noqa: E402
import stress  # noqa: E402

VARIANTS = ["grid", "list", "upnext", "mini"]
# tauri.conf.json minWidth/minHeight, then the sizes people commonly use.
SIZES = [
    (520, 400),
    (900, 600),
    (1024, 640),
    (1200, 630),
    (1190, 1880),
    (1440, 900),
    (1920, 1080),
    (2560, 1440),
]
MINI_SIZES = [(280, 84), (320, 96), (480, 160)]
IGNORE = ".search-popup, .fsp-backdrop, .page-backdrop"

artists, albums, tracks = library.build()
stress.add(artists, albums, tracks)
ALBUMS = render.clean(albums)
ARTISTS = render.clean(artists)
for a in ARTISTS:
    a.pop("genre", None)

PLAYLISTS = library.PLAYLISTS + [(stress.LONG_PLAYLIST, 8), ("A", 2)]
FEATURED = [
    "Degenerative Pixels",
    stress.LONG_ARTIST,
    stress.UNBROKEN,
    "Ox",
    "Nameless Cartographers",
    "Ångström Ünïcode 音楽",
]
STRESS_TRACKS = [t for t in tracks if t["artistName"] in FEATURED]
QUEUE_TRACKS = STRESS_TRACKS[:6] + tracks[:6]
PLAYING = STRESS_TRACKS[0]
MAC_UA = render.MAC_UA


def short(name):
    return re.sub(r"[^a-z0-9]+", "-", name.lower())[:18].strip("-")


def playlist_tracks(i):
    pool = [t for t in tracks if t["artistName"] in FEATURED] + tracks[: 20 + i]
    return [
        {
            "jellyfinId": t["jellyfinId"],
            "playlistItemId": f"pi{i}-{n}",
            "name": t["name"],
            "artistName": t["artistName"],
            "albumName": t["albumName"],
            "albumJellyfinId": t["albumJellyfinId"],
            "durationMs": t["durationMs"],
            "artworkUrl": t.get("artworkUrl"),
        }
        for n, t in enumerate(pool[: PLAYLISTS[i][1]])
    ]


def genre_stats():
    out = {}
    for t in tracks:
        g = out.setdefault(
            t["genre"],
            {"tracks": 0, "artists": set(), "albums": [], "ms": 0, "years": []},
        )
        g["tracks"] += 1
        g["artists"].add(t["artistName"])
        g["ms"] += t["durationMs"]
        g["years"].append(t["year"])
        if t["albumJellyfinId"] not in g["albums"]:
            g["albums"].append(t["albumJellyfinId"])
    return [
        {
            "name": n,
            "trackCount": g["tracks"],
            "artistCount": len(g["artists"]),
            "albumCount": len(g["albums"]),
            "durationMs": g["ms"],
            "yearMin": min(g["years"]),
            "yearMax": max(g["years"]),
            "albumIds": g["albums"][:4],
        }
        for n, g in sorted(out.items())
    ] + [
        {
            "name": "An Extremely Long Genre Name for Layout Testing Purposes",
            "trackCount": 3,
            "artistCount": 1,
            "albumCount": 1,
            "durationMs": 600000,
            "yearMin": 2020,
            "yearMax": 2020,
            "albumIds": [],
        }
    ]


def downloads():
    states = [
        ("downloading", 40),
        ("pending", 0),
        ("pending", 0),
        ("failed", 12),
        ("completed", 100),
        ("downloading", 80),
        ("pending", 0),
        ("cancelled", 0),
        ("pending", 0),
        ("pending", 0),
    ]
    return [
        {
            "trackId": t["jellyfinId"],
            "trackName": t["name"],
            "artistName": t["artistName"],
            "albumId": t["albumJellyfinId"],
            "status": s,
            "progressPercent": p,
            "bytesDownloaded": int(t["fileSizeBytes"] * p / 100),
            "totalBytes": t["fileSizeBytes"],
            "queuedAt": 1_760_000_000,
            **(
                {"error": "Server returned 500 while fetching the stream"}
                if s == "failed"
                else {}
            ),
        }
        for t, (s, p) in zip(STRESS_TRACKS[:8] + tracks[:2], states)
    ]


# Commands the demo library does not answer (render.HANDLERS does the rest).
# Static answers; the ones that read the library are EXTRA_JS below.
INVOKE = {
    "get_genre_stats": genre_stats(),
    "get_download_queue": downloads(),
    "get_cache_stats": {
        "albumCount": 40,
        "trackCount": 400,
        "artistCount": 20,
        "pendingSyncCount": 0,
    },
    "get_app_info": {
        "version": "2026.10.9",
        "tauriVersion": "2.11.0",
        "platform": "macos",
        "arch": "aarch64",
        "dataDir": "/data/net.example.bragi/with/a/long/path",
    },
    "get_developer_info": {
        "databasePath": "/data/net.example.bragi/bragi.db",
        "databaseSizeBytes": 52428800,
        "cacheDir": "/data/net.example.bragi/audio-cache",
        "debugMode": False,
    },
    "get_output_devices": [
        {"name": "MacBook Pro Speakers", "isDefault": True},
        {
            "name": "External Studio Monitors (USB Audio Class 2.0 Interface)",
            "isDefault": False,
        },
    ],
    "get_eq_presets": [
        {"name": "Flat", "isCustom": False},
        {"name": "Bass Boost", "isCustom": False},
    ],
}

EXTRA_JS = r"""
(() => {
  const D = window.__DEMO__;
  const hit = (q, s) => s.toLowerCase().includes(q.toLowerCase());
  const fns = {
    get_recently_added_albums: () => D.albums.slice().sort((a, b) => b.dateCreated - a.dateCreated).slice(0, 40),
    get_jellyfin_playlist: a => ({ jellyfinId: a.playlistId, tracks: D.playlistTracks[a.playlistId] || [] }),
    get_cached_jellyfin_playlist: a => ({ jellyfinId: a.playlistId, tracks: D.playlistTracks[a.playlistId] || [] }),
    search_library: a => ({
      tracks: D.tracks.filter(t => hit(a.query, t.name)).slice(0, 25),
      albums: D.albums.filter(x => hit(a.query, x.name) || hit(a.query, x.artistName)),
      artists: D.artists.filter(x => hit(a.query, x.name)),
    }),
    get_track_lyrics: () => ({ synced: true, lines: Array.from({ length: 24 }, (_, i) => ({ startMs: i * 4000, text: i % 5 === 0 ? 'A very long lyric line that keeps going well past the width of a narrow lyrics column' : 'Short line ' + i })) }),
    get_track_waveform: () => Array.from({ length: 200 }, (_, i) => Math.abs(Math.sin(i / 7)) * 0.8 + 0.1),
  };
  window.__MOCK_INVOKE__ = Object.assign({}, window.__demoHandlers, window.__MOCK_INVOKE__, fns);
})();
"""


def settings_for(variant, zoom):
    # Bragi applies zoom itself from uiZoomLevel, so the sweep sets it here and
    # set_zoom below leaves the root alone.
    s = {
        "uiZoomLevel": zoom,
        "recentlyAddedDays": 365,
        "queueSidebarVisible": variant == "upnext",
        "columnBrowserVisible": False,
        "columnBrowserHeight": 200,
    }
    if variant == "list":
        s.update(viewMode="list", artistViewMode="list", genreViewMode="list")
    entries = [
        {"track": t, "playedAt": 1_760_000_000_000 - i * 3_600_000}
        for i, t in enumerate(STRESS_TRACKS[:10] + tracks[:6])
    ]
    s["recentlyPlayedJson"] = json.dumps(entries)
    return s


def init_script(variant, zoom):
    data = {
        "albums": ALBUMS,
        "artists": ARTISTS,
        "tracks": tracks,
        "playlists": [
            {"jellyfinId": f"pl{i}", "name": n, "itemCount": c}
            for i, (n, c) in enumerate(PLAYLISTS)
        ],
        "playlistTracks": {f"pl{i}": playlist_tracks(i) for i in range(len(PLAYLISTS))},
        "settings": settings_for(variant, zoom),
    }
    label = (
        "window.__TAURI_INTERNALS__.metadata.currentWindow.label = 'mini';"
        if variant == "mini"
        else ""
    )
    return "\n".join(
        [
            f"window.__DEMO__ = {json.dumps(data)};",
            # The app reads the OS from the user agent; the sweep runs on Linux.
            f"Object.defineProperty(navigator, 'userAgent', {{ get: () => {json.dumps(MAC_UA)} }});",
            render.HANDLERS,
            EXTRA_JS,
            label,
        ]
    )


def set_zoom(page, zoom):
    """Zoom arrives through uiZoomLevel in init_script; zooming the root as well would apply it twice."""


def serve_art(route):
    fam = {a["jellyfinId"]: (a["family"], a["name"]) for a in artists}
    album_info = {a["jellyfinId"]: a for a in albums}
    key = route.request.url.rsplit("/", 1)[1].rsplit(".", 1)[0]
    if key in album_info:
        a = album_info[key]
        body = art.cover(a["family"], a["name"], a["artistName"])
    elif key in fam:
        body = art.artist_image(*fam[key])
    else:
        return route.fulfill(status=404)
    route.fulfill(
        status=200,
        content_type="image/svg+xml",
        body=body,
        headers={"Access-Control-Allow-Origin": "*"},
    )


def ready(page, variant):
    # Artwork URLs are http://demo.art/...; route them, then reload so the first
    # paint already has covers (init scripts persist across the reload).
    page.context.route("http://demo.art/**", serve_art)
    page.reload(wait_until="networkidle")
    page.wait_for_function(
        "window.__tauriListeners().includes('playback-state-changed')"
    )
    page.evaluate(
        """([q, t]) => {
          window.__tauriEmit('queue-changed', { type: 'queueChanged', tracks: q, currentIndex: 0, sources: ['context'] });
          window.__tauriEmit('playback-state-changed', { type: 'stateChanged', currentTrack: t, isPlaying: true,
            currentTimeMs: 72000, durationMs: t.durationMs, volume: 0.7, shuffle: false, repeat: 'off', outputDevice: null });
        }""",
        [QUEUE_TRACKS, PLAYING],
    )
    page.wait_for_timeout(700)


def nav(page, name):
    page.get_by_role(
        "button", name=re.compile(rf"^{re.escape(name)}( \d+)?$")
    ).first.click()
    page.wait_for_timeout(600)


def open_artist(page, name):
    nav(page, "Albums")
    nav(page, "Artists")
    page.get_by_text(name, exact=True).first.click()
    page.wait_for_timeout(900)


def open_album(page):
    nav(page, "Albums")
    page.get_by_text("Night Bloom", exact=True).first.click()
    page.wait_for_timeout(900)


def open_playlist(page, i):
    name = PLAYLISTS[i][0]
    try:
        page.get_by_text(name, exact=True).first.click(timeout=2000)
    except Exception:
        nav(page, "Playlists")
        page.get_by_text(name, exact=True).first.click(timeout=2000)
    page.wait_for_timeout(900)


def search(page):
    nav(page, "Albums")
    page.get_by_role("button", name="Search library").first.click()
    page.get_by_placeholder("Search library...").fill("the")
    page.wait_for_timeout(1200)


def open_settings(page, pane):
    gear = page.locator('button[aria-label="Settings"]:visible')
    if gear.count():
        gear.first.click()
    else:
        # Compact top bar: the gear lives in the More controls menu.
        page.get_by_role("button", name="More controls").click()
        page.locator(".overflow-option:visible", has_text="Settings").first.click()
    page.wait_for_timeout(500)
    page.get_by_role("button", name=pane, exact=True).first.click()
    page.wait_for_timeout(500)


def fullscreen(page):
    page.get_by_role("button", name="Open full-screen player").first.click()
    page.wait_for_timeout(1200)


def mini_size(page, size):
    page.set_viewport_size({"width": size[0], "height": size[1]})
    page.wait_for_timeout(400)


def views(variant):
    """(name, action) pairs for one variant. Actions start from any state."""
    v = []
    if variant == "grid":
        v += [
            ("albums-grid", lambda p: nav(p, "Albums")),
            ("artists-grid", lambda p: nav(p, "Artists")),
        ]
        v += [
            (f"artist-hero-{short(n)}", lambda p, n=n: open_artist(p, n))
            for n in FEATURED
        ]
        v += [
            ("genres-grid", lambda p: nav(p, "Genres")),
            ("songs", lambda p: nav(p, "Songs")),
            ("album-detail", open_album),
            ("recently-added", lambda p: nav(p, "Recently Added")),
            ("recently-played", lambda p: nav(p, "Recently Played")),
            ("download-queue", lambda p: nav(p, "Queue")),
        ]
        v += [
            (f"playlist-{i}", lambda p, i=i: open_playlist(p, i))
            for i in range(len(PLAYLISTS))
        ]
        v += [("search-results", search)]
        v += [
            (f"settings-{s}", lambda p, s=s: open_settings(p, s))
            for s in (
                "General",
                "Playback",
                "Library",
                "Downloads",
                "Services",
                "Advanced",
                "About",
            )
        ]
        v += [("fullscreen-player", fullscreen)]
    elif variant == "list":
        v += [
            ("albums-list", lambda p: nav(p, "Albums")),
            ("artists-list", lambda p: nav(p, "Artists")),
        ]
        v += [
            (f"artist-split-{short(n)}", lambda p, n=n: open_artist(p, n))
            for n in FEATURED
        ]
        v += [("genres-list", lambda p: nav(p, "Genres"))]
    elif variant == "upnext":
        v += [("up-next-panel", lambda p: nav(p, "Albums"))]
    elif variant == "mini":
        v += [
            (f"mini-player-{w}x{h}", lambda p, s=(w, h): mini_size(p, s))
            for w, h in MINI_SIZES
        ]
    return v
