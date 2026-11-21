"""Stress rows for the layout sweep: names that break naive layouts.

Appended to the demo library by sweep.py only; the README renders do not see them.
An item with `noart` has no artworkUrl, so the app shows its placeholder.
"""

LONG_ARTIST = "The Incredibly Long-Winded Philharmonic Orchestra and Chamber Choir of the Greater Metropolitan Region feat. Special Guests"
LONG_ALBUM = "A Very Long Album Title That Goes On and On: Live at the Royal Concert Hall (Deluxe Remastered Anniversary Edition)"
UNBROKEN = "Pneumonoultramicroscopicsilicovolcanoconiosis"
LONG_TRACK = "An Exceedingly Verbose Track Title With Several Parenthetical Asides (Reprise) (Live at the Royal Concert Hall) [2024 Remaster]"

# (artist, genre, art family or None for no art, [(album, year, [track titles])])
STRESS = [
    (
        "Degenerative Pixels",
        "Glitch",
        "rings",
        [
            (
                "Dead Pixel Cartography",
                2023,
                ["Stuck Red", "Burn In", "Ghost Image", "Dithering"],
            )
        ],
    ),
    (
        LONG_ARTIST,
        "Classical",
        "haze",
        [
            (
                LONG_ALBUM,
                2022,
                [
                    LONG_TRACK,
                    "Short",
                    "Allegro",
                    "Largo",
                    "Adagio Molto Cantabile e Sostenuto",
                ],
            )
        ],
    ),
    (
        UNBROKEN,
        "Experimental",
        "blocks",
        [(f"{UNBROKEN}_{UNBROKEN}", 2020, [UNBROKEN, "Ok"])],
    ),
    ("Ox", "Punk", "sigil", [("X", 2019, ["I", "II", "III"])]),
    ("A", "Noise", "arches", [("B", 2018, ["C", "D"])]),
    (
        "Nameless Cartographers",
        "Folk",
        None,
        [
            ("Unmarked Territory", 2021, ["Blank Page", "No Cover Art"]),
            ("Also Unmarked", 2024, ["Void"]),
        ],
    ),
    (
        "Ångström Ünïcode 音楽",
        "World",
        "landscape",
        [("Ōsaka 大阪 — Ñandú", 2017, ["Café Zürich", "日本語のタイトル"])],
    ),
]

LONG_PLAYLIST = "Everything I Want To Hear On A Very Long Road Trip Across The Nullarbor Plain With Friends"


def add(artists, albums, tracks):
    """Append the stress rows to the demo rows in place."""
    base_ts = 1_760_000_000 + 86400
    for si, (name, genre, family, discography) in enumerate(STRESS):
        aid = f"st{si:02d}"
        art = family is not None
        artists.append(
            {
                "jellyfinId": aid,
                "name": name,
                "albumCount": len(discography),
                **({"artworkUrl": f"http://demo.art/{aid}.svg"} if art else {}),
                "sortName": name,
                "family": family or "rings",
                "genre": genre,
            }
        )
        for bi, (title, year, titles) in enumerate(discography):
            lid = f"stal{si:02d}{bi}"
            ts = []
            for i, t in enumerate(titles, 1):
                d = (150 + 37 * i) * 1000
                ts.append(
                    {
                        "jellyfinId": f"{lid}-t{i:02d}",
                        "name": t,
                        "artistName": name,
                        "albumName": title,
                        "albumJellyfinId": lid,
                        "trackNumber": i,
                        "discNumber": 1,
                        "durationMs": d,
                        "fileSizeBytes": d * 38,
                        "genre": genre,
                        "year": year,
                        **({"artworkUrl": f"http://demo.art/{lid}.svg"} if art else {}),
                    }
                )
            tracks.extend(ts)
            albums.append(
                {
                    "jellyfinId": lid,
                    "name": title,
                    "artistName": name,
                    "artistJellyfinId": aid,
                    "year": year,
                    "genre": genre,
                    **({"artworkUrl": f"http://demo.art/{lid}.svg"} if art else {}),
                    "trackCount": len(ts),
                    "durationMs": sum(x["durationMs"] for x in ts),
                    "downloadedTrackCount": 0,
                    "dateCreated": base_ts - si * 86400,
                    "family": family or "rings",
                }
            )
