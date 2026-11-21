# Null MBID Troubleshooting

Symptom: a track that _is_ MusicBrainz-tagged in its file won't scrobble from Bragi; the queue payload shows `trackMbid: null`.

**Root cause is almost always stale Jellyfin metadata, not an untagged file.** Jellyfin reads track MBIDs from embedded file tags on scan. An item scanned before the file carried a MusicBrainz **recording** id has only `ProviderIds.MusicBrainzTrack` (the release-track id), never `MusicBrainzRecording`. Bragi keys on `MusicBrainzRecording` (P09), so it reads null; recently-rescanned items have both.

Diagnose:

1. File tag truth: check `musicbrainz_trackid` on the actual file (mutagen). If present, the file is fine.
2. Jellyfin truth: `jellyfin` MCP `library get item_id=<id> verbose=true` -> `ProviderIds`. Missing `MusicBrainzRecording` but present `MusicBrainzTrack` confirms stale metadata.

Fix: force Jellyfin to re-read embedded ids. A plain refresh does not; a **FullRefresh + replaceAllMetadata** does — `POST /Items/<id>/Refresh?metadataRefreshMode=FullRefresh&imageRefreshMode=None&replaceAllMetadata=true`, once per stale album (cascades to its tracks).

Trap: "not in beets library" is not "untagged" — beets imports at album granularity and skips compilations it can't strong-match, even when the files are fully Picard-tagged. Measure by the file's recording-MBID tag, never by beets membership. The library is tagged with Picard; beets is a secondary auto-importer for new arrivals only.

Bragi self-heals this: a null-MBID listen is re-queried live against Jellyfin on each drain and scrobbled with the id once Jellyfin exposes it. If Jellyfin still has none (a live recording with no MusicBrainz entry), the listen is sent by artist and title over `/1/submit-listens`, where the scrobble server mints a stable id for the normalised triple; `/client/sync` would reject it (`rejected_no_mbid`). A skip with no id cannot be recorded and is dropped.
