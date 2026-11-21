#!/usr/bin/env python3
"""
Audit audio files for MusicBrainz Track IDs.

Scans a music directory and reports which files are missing MusicBrainz track IDs,
grouped by artist for easy identification.

Usage:
    python audit_musicbrainz_ids.py [music_directory]
    python audit_musicbrainz_ids.py -v  # Verbose - print as files are found

If no directory is specified, defaults to the Jellyfin music library path.

Requirements:
    nix shell nixpkgs#python312Packages.mutagen
"""

import argparse
import os
import sys
from collections import defaultdict
from pathlib import Path

try:
    from mutagen import File
except ImportError:
    print(
        "ERROR: mutagen not installed. Run: nix shell nixpkgs#python312Packages.mutagen"
    )
    sys.exit(1)

DEFAULT_MUSIC_DIR = "~/Library/Application Support/Mountain Duck/Volumes.noindex/reservoir.localized/servarr/media/music"

AUDIO_EXTENSIONS = {".mp3", ".flac", ".m4a", ".ogg", ".opus", ".wma", ".wav"}


def has_musicbrainz_track_id(audio, ext: str) -> bool:
    """Check if an audio file has a MusicBrainz Track ID tag."""
    if audio is None or audio.tags is None:
        return False

    tags = audio.tags

    if ext == ".mp3":
        # ID3 tags - check various possible tag names
        for key in tags.keys():
            key_lower = key.lower()
            if "ufid:http://musicbrainz.org" in key_lower:
                return True
            if "musicbrainz release track id" in key_lower:
                return True
            if "musicbrainz_trackid" in key_lower:
                return True
        return False

    elif ext == ".flac":
        # Vorbis comments
        return "musicbrainz_trackid" in [k.lower() for k in tags.keys()]

    elif ext == ".m4a":
        # MP4 tags
        return "----:com.apple.iTunes:MusicBrainz Track Id" in tags

    elif ext in {".ogg", ".opus"}:
        # Vorbis comments
        return "musicbrainz_trackid" in [k.lower() for k in tags.keys()]

    return False


def count_audio_files(music_dir: Path) -> int:
    """Count total audio files for progress reporting."""
    count = 0
    for root, _dirs, files in os.walk(music_dir):
        for f in files:
            if Path(f).suffix.lower() in AUDIO_EXTENSIONS:
                count += 1
    return count


def audit_music_directory(
    music_dir: Path, verbose: bool = False
) -> tuple[list[str], int, list[str]]:
    """
    Audit a music directory for MusicBrainz Track IDs.

    Args:
        music_dir: Path to the music directory
        verbose: If True, print missing files as they are found

    Returns:
        Tuple of (missing_mbid_paths, count_with_mbid, errors)
    """
    missing_mbid = []
    has_mbid = 0
    errors = []
    processed = 0

    # Count total files for progress
    print("Counting files...", file=sys.stderr)
    sys.stderr.flush()
    total_files = count_audio_files(music_dir)
    print(f"Found {total_files} audio files to scan\n", file=sys.stderr)
    sys.stderr.flush()

    for root, _dirs, files in os.walk(music_dir):
        for filename in files:
            ext = Path(filename).suffix.lower()
            if ext not in AUDIO_EXTENSIONS:
                continue

            filepath = Path(root) / filename
            relative_path = filepath.relative_to(music_dir)
            processed += 1

            # Progress indicator every 50 files
            if processed % 50 == 0:
                pct = (processed / total_files) * 100
                print(
                    f"  [{processed:>5}/{total_files}] {pct:5.1f}% - {str(relative_path)[:60]}",
                    file=sys.stderr,
                )
                sys.stderr.flush()

            try:
                audio = File(filepath)
                if audio is None:
                    errors.append(f"Could not read: {relative_path}")
                    continue

                if has_musicbrainz_track_id(audio, ext):
                    has_mbid += 1
                else:
                    missing_mbid.append(str(relative_path))
                    if verbose:
                        print(f"MISSING: {relative_path}")
                        sys.stdout.flush()

            except Exception as e:
                errors.append(f"Error reading {relative_path}: {e}")

    print(f"\n  Complete! Processed {processed} files.", file=sys.stderr)
    sys.stderr.flush()
    return missing_mbid, has_mbid, errors


def group_by_artist(paths: list[str]) -> dict[str, list[str]]:
    """Group file paths by artist (first directory component)."""
    by_artist = defaultdict(list)
    for path in paths:
        parts = path.split("/")
        if len(parts) >= 2:
            artist = parts[0]
            by_artist[artist].append(path)
        else:
            by_artist["(root)"].append(path)
    return dict(by_artist)


def main():
    parser = argparse.ArgumentParser(
        description="Audit audio files for MusicBrainz Track IDs"
    )
    parser.add_argument(
        "music_dir",
        nargs="?",
        default=DEFAULT_MUSIC_DIR,
        help=f"Music directory to scan (default: {DEFAULT_MUSIC_DIR})",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output results as JSON",
    )
    parser.add_argument(
        "-v",
        "--verbose",
        action="store_true",
        help="Print missing files as they are found",
    )
    args = parser.parse_args()

    music_dir = Path(args.music_dir)
    if not music_dir.exists():
        print(f"ERROR: Directory does not exist: {music_dir}")
        sys.exit(1)

    print(f"Scanning: {music_dir}\n", file=sys.stderr)
    sys.stderr.flush()

    missing_mbid, has_mbid, errors = audit_music_directory(
        music_dir, verbose=args.verbose
    )

    if args.json:
        import json

        result = {
            "total_with_mbid": has_mbid,
            "total_missing_mbid": len(missing_mbid),
            "total_errors": len(errors),
            "missing_by_artist": group_by_artist(missing_mbid),
            "errors": errors[:50],  # Limit errors in JSON output
        }
        print(json.dumps(result, indent=2))
    else:
        print(f"\n=== MusicBrainz Track ID Audit ===\n")
        print(f"Total files with MBID: {has_mbid}")
        print(f"Total files MISSING MBID: {len(missing_mbid)}")
        print(f"Errors: {len(errors)}")

        if missing_mbid:
            print(f"\n=== Files Missing MusicBrainz Track ID ===")
            by_artist = group_by_artist(missing_mbid)

            for artist in sorted(by_artist.keys()):
                print(f"\n{artist}:")
                for path in sorted(by_artist[artist]):
                    # Show path without artist prefix
                    display_path = "/".join(path.split("/")[1:])
                    print(f"  - {display_path}")

        if errors:
            print(f"\n=== Errors ===\n")
            for err in errors[:20]:
                print(f"  {err}")
            if len(errors) > 20:
                print(f"  ... and {len(errors) - 20} more errors")


if __name__ == "__main__":
    main()
