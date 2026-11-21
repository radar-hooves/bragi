"""Invented demo library for the README screenshots.

Every artist and album here is fictional. Track titles are assembled from
per-genre word lists, seeded by the album title so the library is stable.
"""

import hashlib
import random

# (artist, genre, art family, [(album, year), ...])
ARTISTS = [
    (
        "Basalt Throne",
        "Heavy Metal",
        "sigil",
        [("Iron Lung of the Sky", 2019), ("Obsidian Procession", 2022)],
    ),
    (
        "Brassline Orchestra",
        "Jazz",
        "blocks",
        [
            ("Night Shift at the Foundry", 2016),
            ("Seven Ways to Leave a Ballroom", 2021),
        ],
    ),
    (
        "Cobalt Husk",
        "Industrial",
        "rings",
        [("Hollow Machinery", 2013), ("Rust Choir", 2018)],
    ),
    (
        "Concrete Almanac",
        "Hip-Hop",
        "halftone",
        [("Block Party Weather", 2017), ("Ledger of Small Wins", 2023)],
    ),
    (
        "Copper Lantern Quartet",
        "Jazz",
        "blocks",
        [("Lamplight Standards", 2011), ("Quarter Past Blue", 2020)],
    ),
    (
        "Dusty Algebra",
        "Lo-Fi",
        "halftone",
        [("Rainy Study Tapes", 2020), ("Proofs and Pancakes", 2024)],
    ),
    (
        "Ferrymen of Dusk",
        "Jazz",
        "blocks",
        [("Crossing at Low Tide", 2014), ("The Last Boat Home", 2019)],
    ),
    (
        "Finch and Fathom",
        "Folk",
        "landscape",
        [("Kitchen Table Hymns", 2015), ("Salt on the Windowsill", 2022)],
    ),
    (
        "Gilded Anchor",
        "Soul",
        "blocks",
        [("Harbour Lights", 2018), ("Slow Dance in the Rain", 2024)],
    ),
    (
        "Glasshouse Static",
        "Electronic",
        "arches",
        [("Fractal Lullabies", 2012), ("Condensation", 2021)],
    ),
    (
        "Grim Lantern",
        "Black Metal",
        "sigil",
        [("Pale Fire Liturgy", 2017), ("Winter Has No Witnesses", 2023)],
    ),
    (
        "Hollow Meridian",
        "Ambient",
        "haze",
        [("Slow Weather", 2016), ("Tidal Archive", 2023)],
    ),
    (
        "Hushfall",
        "Neoclassical",
        "haze",
        [("Snow in the Concert Hall", 2019), ("Small Rooms", 2022)],
    ),
    (
        "Ironbark Ramblers",
        "Country",
        "landscape",
        [("Dust Road Gospel", 2014), ("Fiddle in the Fog", 2021)],
    ),
    (
        "Kite Weather",
        "Indie Pop",
        "arches",
        [("Paper Planets", 2018), ("Weekend Forecast", 2024)],
    ),
    (
        "Lowlight Parade",
        "Shoegaze",
        "haze",
        [("Bloom Filter", 2015), ("Soft Collisions", 2020)],
    ),
    (
        "Lunar Freight",
        "Techno",
        "rings",
        [("Cargo Bay Rhythms", 2019), ("Zero-G Warehouse", 2023)],
    ),
    (
        "Mirror Tide",
        "House",
        "rings",
        [("Sunset Terminal", 2017), ("Open Windows, Closed Eyes", 2022)],
    ),
    (
        "Moth Cathedral",
        "Post-Rock",
        "haze",
        [("Slow Burn Sanctuary", 2013), ("Stained Glass Weather", 2019)],
    ),
    (
        "Neon Orchard",
        "Synthwave",
        "sunset",
        [("Night Bloom", 2016), ("Sodium Dreams", 2021)],
    ),
    (
        "Orbit Kitchen",
        "Hip-Hop",
        "halftone",
        [("Zero Gravity Breakfast", 2019), ("Late Orders", 2024)],
    ),
    (
        "Pale Cartwheel",
        "Dream Pop",
        "haze",
        [("Honey Static", 2017), ("Summer Under Glass", 2023)],
    ),
    (
        "Rust Belt Hymnal",
        "Americana",
        "landscape",
        [("Factory Whistle Blues", 2012), ("Gospel for Empty Towns", 2018)],
    ),
    (
        "Sable Harbour",
        "Blues Rock",
        "arches",
        [("Low Tide Boogie", 2015), ("Lighthouse Fever", 2022)],
    ),
    (
        "Saltmarsh Choir",
        "Folk",
        "landscape",
        [("Reed Songs", 2014), ("Wading Songs", 2021)],
    ),
    (
        "Static Meadow",
        "Post-Rock",
        "haze",
        [("Field Recordings for Machines", 2018), ("Grass Frequency", 2023)],
    ),
    (
        "Sunday Tarmac",
        "Punk",
        "halftone",
        [("Short Fuse Summer", 2016), ("Loud Neighbours", 2022)],
    ),
    (
        "Tallow and Ember",
        "Doom Metal",
        "sigil",
        [("Candle Funeral", 2015), ("Slow Ash", 2021)],
    ),
    (
        "Tundra Postcards",
        "Chillwave",
        "sunset",
        [("Glacier Mail", 2017), ("Wish You Were Frozen", 2022)],
    ),
    (
        "Velvet Cartographers",
        "Indie Rock",
        "arches",
        [("Maps for Rooms", 2014), ("Late Edition", 2021)],
    ),
]

PLAYLISTS = [("Rainy Day Drive", 14), ("Focus Hours", 22), ("Sunday Kitchen", 18)]

WORDS = {
    "default": (
        [
            "Silver",
            "Quiet",
            "Paper",
            "Slow",
            "Golden",
            "Hollow",
            "Distant",
            "Open",
            "Burning",
            "Winter",
            "Second",
            "Last",
            "Blue",
            "Glass",
            "Northern",
            "Wild",
        ],
        [
            "Harbour",
            "Signal",
            "Garden",
            "Engine",
            "Letter",
            "River",
            "Season",
            "Window",
            "Orbit",
            "Weather",
            "Morning",
            "Parade",
            "Lantern",
            "Compass",
            "Echo",
            "Tide",
        ],
    ),
    "metal": (
        [
            "Iron",
            "Black",
            "Ashen",
            "Fallen",
            "Burning",
            "Hollow",
            "Cold",
            "Bleeding",
            "Sunken",
            "Dead",
        ],
        [
            "Throne",
            "Procession",
            "Altar",
            "Crown",
            "Hymn",
            "Siege",
            "Vigil",
            "Ritual",
            "Temple",
            "Harvest",
            "Winter",
        ],
    ),
    "hiphop": (
        [
            "Late",
            "Corner",
            "Cold",
            "Heavy",
            "Small",
            "Midnight",
            "Loose",
            "Brick",
            "Paper",
            "Slow",
        ],
        [
            "Orders",
            "Ledger",
            "Blocks",
            "Receipts",
            "Hustle",
            "Weather",
            "Cipher",
            "Rooftop",
            "Breakfast",
            "Frequency",
            "Rotation",
        ],
    ),
    "electronic": (
        [
            "Neon",
            "Zero",
            "Phase",
            "Sodium",
            "Digital",
            "Cargo",
            "Chrome",
            "Static",
            "Binary",
            "Night",
        ],
        [
            "Bloom",
            "Terminal",
            "Loop",
            "Garden",
            "Drift",
            "Circuit",
            "Pulse",
            "Horizon",
            "Lattice",
            "Signal",
            "Transit",
        ],
    ),
    "ambient": (
        [
            "Slow",
            "Still",
            "Pale",
            "Distant",
            "Soft",
            "Falling",
            "Drifting",
            "Low",
            "Quiet",
            "Faded",
        ],
        [
            "Weather",
            "Light",
            "Archive",
            "Snow",
            "Room",
            "Tide",
            "Fog",
            "Meadow",
            "Glass",
            "Hours",
            "Horizon",
        ],
    ),
    "folk": (
        [
            "Salt",
            "Harvest",
            "Kitchen",
            "Copper",
            "Old",
            "Reed",
            "Wooden",
            "Morning",
            "Winter",
            "Borrowed",
        ],
        [
            "Hymn",
            "Table",
            "Orchard",
            "Road",
            "Song",
            "Lantern",
            "River",
            "Valley",
            "Window",
            "Ballad",
            "Fiddle",
        ],
    ),
    "jazz": (
        [
            "Blue",
            "Late",
            "Midnight",
            "Quarter",
            "Brass",
            "Slow",
            "Seventh",
            "Lamplight",
            "Back",
            "Corner",
        ],
        [
            "Standard",
            "Waltz",
            "Shuffle",
            "Ballroom",
            "Hour",
            "Room",
            "Chorus",
            "Street",
            "Changes",
            "Turnaround",
        ],
    ),
}
FAMILY_WORDS = {
    "sigil": "metal",
    "halftone": "hiphop",
    "sunset": "electronic",
    "rings": "electronic",
    "haze": "ambient",
    "landscape": "folk",
    "blocks": "jazz",
    "arches": "default",
}


def _rng(*parts):
    h = hashlib.sha256("|".join(parts).encode()).digest()
    return random.Random(int.from_bytes(h[:8], "big"))


def _tracks(album_id, artist, album, genre, year, family):
    rng = _rng("tracks", album)
    adjs, nouns = WORDS[FAMILY_WORDS[family]]
    count = rng.randint(8, 12)
    titles, seen = [], set()
    while len(titles) < count:
        shape = rng.random()
        if shape < 0.5:
            t = f"{rng.choice(adjs)} {rng.choice(nouns)}"
        elif shape < 0.75:
            t = f"{rng.choice(nouns)} of the {rng.choice(nouns)}"
        elif shape < 0.9:
            t = f"{rng.choice(adjs)} {rng.choice(nouns)} {rng.choice(['I', 'II', 'III'])}"
        else:
            t = rng.choice(nouns)
        if t not in seen and t != album:
            seen.add(t)
            titles.append(t)
    lo, hi = {
        "ambient": (240, 520),
        "haze": (220, 480),
        "sigil": (210, 450),
        "rings": (260, 440),
    }.get(family, (150, 330))
    out = []
    for i, t in enumerate(titles, 1):
        out.append(
            {
                "jellyfinId": f"{album_id}-t{i:02d}",
                "name": t,
                "artistName": artist,
                "albumName": album,
                "albumJellyfinId": album_id,
                "trackNumber": i,
                "discNumber": 1,
                "durationMs": (d := rng.randint(lo, hi) * 1000 + rng.randint(0, 999)),
            "fileSizeBytes": d * 38,
                "genre": genre,
                "year": year,
                "artworkUrl": f"http://demo.art/{album_id}.svg",
            }
        )
    return out


def build():
    artists, albums, tracks = [], [], []
    base_ts = 1_760_000_000
    n = 0
    for ai, (name, genre, family, discography) in enumerate(ARTISTS):
        aid = f"ar{ai:02d}"
        artists.append(
            {
                "jellyfinId": aid,
                "name": name,
                "albumCount": len(discography),
                "artworkUrl": f"http://demo.art/{aid}.svg",
                "sortName": name,
                "family": family,
                "genre": genre,
            }
        )
        for bi, (title, year) in enumerate(discography):
            lid = f"al{ai:02d}{bi}"
            t = _tracks(lid, name, title, genre, year, family)
            tracks.extend(t)
            albums.append(
                {
                    "jellyfinId": lid,
                    "name": title,
                    "artistName": name,
                    "artistJellyfinId": aid,
                    "year": year,
                    "genre": genre,
                    "artworkUrl": f"http://demo.art/{lid}.svg",
                    "trackCount": len(t),
                    "durationMs": sum(x["durationMs"] for x in t),
                    "downloadedTrackCount": 0,
                    "dateCreated": base_ts - (n * 86400 * 3),
                    "family": family,
                }
            )
            n += 1
    return artists, albums, tracks
