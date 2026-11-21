#!/usr/bin/env python3
"""
Clean cruft metadata tags from MP3 files.

Removes Windows Media, Zune, iTunes, and other legacy tags that pollute
music libraries, while preserving essential metadata and MusicBrainz IDs.

Usage:
    python clean_metadata_cruft.py [music_directory]
    python clean_metadata_cruft.py --dry-run  # Preview without changes
    python clean_metadata_cruft.py --artist "Trivium"  # Single artist

Requirements:
    nix shell nixpkgs#python312Packages.mutagen
"""

import argparse
import os
import re
import sys
from pathlib import Path

try:
    from mutagen import File
    from mutagen.id3 import ID3
except ImportError:
    print(
        "ERROR: mutagen not installed. Run: nix shell nixpkgs#python312Packages.mutagen"
    )
    sys.exit(1)

DEFAULT_MUSIC_DIR = "~/Library/Application Support/Mountain Duck/Volumes.noindex/reservoir.localized/servarr/media/music"

AUDIO_EXTENSIONS = {".mp3"}  # Only MP3 for now (ID3 tags)

# Patterns for tags to DELETE
CRUFT_PATTERNS = [
    # Windows Media Player
    r"^PRIV:WM/.*",
    # Zune
    r"^PRIV:Zune.*",
    # iTunes normalisation/gapless
    r"^COMM:iTun.*",
    r"^COMM:iTunes_.*",
    # ReplayGain private tags (redundant)
    r"^PRIV:PeakValue$",
    r"^PRIV:AverageLevel$",
    # RealJukebox
    r"^GEOB:RealJukebox.*",
    # CDTag.com
    r"^PRIV:http://www\.cdtag\.com.*",
    # Encoder info (not useful)
    r"^TENC$",
    r"^TSSE$",
    # Empty/legacy comments
    r"^COMM:ID3v1 Comment.*",
    r"^COMM::.*",  # Empty description comments
    # iTunes advisory (keep the standard one if present)
    r"^TXXX:ITUNESADVISORY$",
]

# Compile patterns for efficiency
CRUFT_REGEXES = [re.compile(p, re.IGNORECASE) for p in CRUFT_PATTERNS]

# Tags to check for "PMEDIA" value pollution
PMEDIA_CHECK_TAGS = ["TIT1", "TCMP", "TXXX:RELEASECOUNTRY"]


def is_cruft_tag(tag_key: str) -> bool:
    """Check if a tag key matches cruft patterns."""
    for regex in CRUFT_REGEXES:
        if regex.match(tag_key):
            return True
    return False


def has_pmedia_value(audio, tag_key: str) -> bool:
    """Check if a tag has 'PMEDIA' as its value."""
    try:
        val = audio.tags.get(tag_key)
        if val is None:
            return False
        val_str = str(val[0] if isinstance(val, list) else val)
        return "PMEDIA" in val_str.upper()
    except (KeyError, IndexError, TypeError):
        return False


def clean_file(filepath: Path, dry_run: bool = False) -> dict:
    """
    Clean cruft tags from a single file.

    Returns dict with stats about what was cleaned.
    """
    result = {
        "cleaned": False,
        "tags_removed": [],
        "error": None,
    }

    try:
        audio = File(filepath)
        if audio is None or audio.tags is None:
            return result

        tags_to_remove = []

        # Check each tag
        for key in list(audio.tags.keys()):
            # Check against cruft patterns
            if is_cruft_tag(key):
                tags_to_remove.append(key)
                continue

            # Check for PMEDIA pollution
            if key in PMEDIA_CHECK_TAGS or key.startswith("TXXX:"):
                if has_pmedia_value(audio, key):
                    tags_to_remove.append(key)

        if tags_to_remove:
            result["tags_removed"] = tags_to_remove
            result["cleaned"] = True

            if not dry_run:
                for tag in tags_to_remove:
                    try:
                        del audio.tags[tag]
                    except KeyError:
                        pass
                audio.save()

    except Exception as e:
        result["error"] = str(e)

    return result


def clean_directory(
    music_dir: Path,
    dry_run: bool = False,
    artist_filter: str | None = None,
    verbose: bool = False,
) -> dict:
    """
    Clean cruft tags from all MP3 files in a directory.

    Returns summary stats.
    """
    stats = {
        "files_scanned": 0,
        "files_cleaned": 0,
        "tags_removed": 0,
        "errors": 0,
        "tag_counts": {},
    }

    # If artist filter, adjust path
    if artist_filter:
        music_dir = music_dir / artist_filter
        if not music_dir.exists():
            print(f"ERROR: Artist directory not found: {music_dir}")
            sys.exit(1)

    print(f"{'[DRY RUN] ' if dry_run else ''}Scanning: {music_dir}\n")

    # Count files first
    total_files = sum(
        1
        for root, _, files in os.walk(music_dir)
        for f in files
        if Path(f).suffix.lower() in AUDIO_EXTENSIONS
    )
    print(f"Found {total_files} MP3 files to process\n")

    for root, _dirs, files in os.walk(music_dir):
        for filename in files:
            ext = Path(filename).suffix.lower()
            if ext not in AUDIO_EXTENSIONS:
                continue

            filepath = Path(root) / filename
            relative_path = filepath.relative_to(music_dir)
            stats["files_scanned"] += 1

            # Progress every 100 files
            if stats["files_scanned"] % 100 == 0:
                pct = (stats["files_scanned"] / total_files) * 100
                print(f"  [{stats['files_scanned']:>5}/{total_files}] {pct:5.1f}%")
                sys.stdout.flush()

            result = clean_file(filepath, dry_run=dry_run)

            if result["error"]:
                stats["errors"] += 1
                if verbose:
                    print(f"  ERROR: {relative_path}: {result['error']}")

            if result["cleaned"]:
                stats["files_cleaned"] += 1
                stats["tags_removed"] += len(result["tags_removed"])

                # Track which tags were removed
                for tag in result["tags_removed"]:
                    # Normalize for counting
                    norm_tag = tag.split(":")[0] if ":" in tag else tag
                    if tag.startswith("PRIV:"):
                        parts = tag.split(":", 2)
                        norm_tag = (
                            f"PRIV:{parts[1].split(':')[0]}" if len(parts) > 1 else tag
                        )
                    stats["tag_counts"][norm_tag] = (
                        stats["tag_counts"].get(norm_tag, 0) + 1
                    )

                if verbose:
                    print(
                        f"  {'Would clean' if dry_run else 'Cleaned'}: {relative_path}"
                    )
                    for tag in result["tags_removed"]:
                        print(f"    - {tag}")

    return stats


def main():
    parser = argparse.ArgumentParser(
        description="Clean cruft metadata tags from MP3 files"
    )
    parser.add_argument(
        "music_dir",
        nargs="?",
        default=DEFAULT_MUSIC_DIR,
        help=f"Music directory to clean (default: {DEFAULT_MUSIC_DIR})",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Preview changes without modifying files",
    )
    parser.add_argument(
        "--artist",
        type=str,
        help="Clean only a specific artist folder",
    )
    parser.add_argument(
        "-v",
        "--verbose",
        action="store_true",
        help="Show detailed output for each file",
    )
    args = parser.parse_args()

    music_dir = Path(args.music_dir)
    if not music_dir.exists():
        print(f"ERROR: Directory does not exist: {music_dir}")
        sys.exit(1)

    stats = clean_directory(
        music_dir,
        dry_run=args.dry_run,
        artist_filter=args.artist,
        verbose=args.verbose,
    )

    # Summary
    print(f"\n{'=' * 60}")
    print(f"{'[DRY RUN] ' if args.dry_run else ''}SUMMARY")
    print(f"{'=' * 60}")
    print(f"Files scanned:  {stats['files_scanned']}")
    print(f"Files cleaned:  {stats['files_cleaned']}")
    print(f"Tags removed:   {stats['tags_removed']}")
    print(f"Errors:         {stats['errors']}")

    if stats["tag_counts"]:
        print(f"\nTags removed by type:")
        for tag, count in sorted(stats["tag_counts"].items(), key=lambda x: -x[1]):
            print(f"  {tag:<40} {count:>6}")

    if args.dry_run and stats["files_cleaned"] > 0:
        print(f"\nRun without --dry-run to apply changes.")


if __name__ == "__main__":
    main()
