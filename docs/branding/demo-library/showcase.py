"""The README's library: real albums whose covers and music are free to publish.

Read from docs/branding/showcase/manifest.json, each album's licence and source
in docs/branding/showcase/CREDITS.md. An artist's image is the cover of their
first album, as a server with no artist photo shows it.
"""

import json
import pathlib

SHOWCASE = pathlib.Path(__file__).resolve().parents[1] / "showcase"
PLAYLISTS = [("Late Night", 24), ("Focus", 41), ("Sunday Morning", 18)]


def build():
    albums_in = json.loads((SHOWCASE / "manifest.json").read_text())["albums"]
    artist_ids, artists, albums, tracks = {}, [], [], []
    base_ts = 1_760_000_000
    for n, a in enumerate(
        sorted(albums_in, key=lambda a: (a["artist"].lower(), a["year"]))
    ):
        if a["artist"] not in artist_ids:
            aid = f"ar{len(artist_ids):02d}"
            artist_ids[a["artist"]] = aid
            artists.append(
                {
                    "jellyfinId": aid,
                    "name": a["artist"],
                    "albumCount": 0,
                    "artworkUrl": f"http://demo.art/{a['id']}.jpg",
                    "sortName": a["artist"],
                }
            )
        aid = artist_ids[a["artist"]]
        next(x for x in artists if x["jellyfinId"] == aid)["albumCount"] += 1
        art = f"http://demo.art/{a['id']}.jpg"
        album_tracks = [
            {
                "jellyfinId": f"{a['id']}-t{t['disc']}{t['n']:02d}",
                "name": t["title"],
                "artistName": a["artist"],
                "albumName": a["title"],
                "albumJellyfinId": a["id"],
                "trackNumber": t["n"],
                "discNumber": t["disc"],
                "durationMs": t["durationSec"] * 1000,
                "fileSizeBytes": t["durationSec"] * 1000 * 38,
                "genre": a["genre"],
                "year": a["year"],
                "artworkUrl": art,
            }
            for t in a["tracks"]
        ]
        tracks.extend(album_tracks)
        albums.append(
            {
                "jellyfinId": a["id"],
                "name": a["title"],
                "artistName": a["artist"],
                "artistJellyfinId": aid,
                "year": a["year"],
                "genre": a["genre"],
                "artworkUrl": art,
                "trackCount": len(album_tracks),
                "durationMs": sum(t["durationMs"] for t in album_tracks),
                "downloadedTrackCount": 0,
                "dateCreated": base_ts - n * 86400 * 3,
            }
        )
    return artists, albums, tracks
