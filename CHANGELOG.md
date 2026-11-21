# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Calendar Versioning](https://calver.org/) (YYYY.M.x).

## [Unreleased]

## [2026.11.1] - 21/11/2026

### Added

- Bragi checks for a new version when it starts and every six hours, offers it in a notification, and installs it and restarts when you say so. Settings > About shows the last check and can check on demand.

### Changed

- AI Playlists offer three providers: Ollama, any OpenAI-compatible endpoint, or Anthropic. A setup on the retired fourth preset moves onto the OpenAI-compatible provider at its own address, model and key, so it keeps working.
- Settings describe the scrobble server Bragi reads play counts from as a self-hosted scrobble server.

### Fixed

- In the column browser, the selected artist or album reads on every accent colour: it takes the accent's tint, as a selected song does, where white text on the solid accent was hard to read.

## [2026.11.0] - 21/11/2026

### Added

- Bragi now reports why playback stopped (the end-of-file reason, stream failures and each recovery step) to its telemetry, so a silent stop shows its cause in your logs. It carries no track, artist or address.

### Changed

- The scrobble queue now deletes entries that have already synced once they are a week old. Nothing read them, and unsynced scrobbles are never touched.
- Credentials for Jellyfin, OpenSubsonic and the scrobble and LLM services now sit in one shared store. Existing logins carry over, so nothing needs re-entering.
- Calls to the LLM service now carry trace spans like every other request, so they show up in telemetry.

### Fixed

- When the player goes quiet while Bragi still thinks a track is playing, the clock no longer counts on over silence. Bragi notices after two idle readings and moves on to the next track.
- When the Mac cannot open the audio output, as happened with AirPods that dropped overnight and came back, Bragi restarts its player and the track plays, rather than failing three times on the same player and stopping with "audio output initialization failed".

## [2026.10.9] - 23/10/2026

### Changed

- Bragi's app identifier is now `com.poodle64.bragi`. On first launch it copies your library database, settings, credentials, window position and artwork from the folders of the previous identifier to the new ones, and leaves the old folders in place, so you can go back to an earlier release. Delete the old folders yourself once you are happy.

### Fixed

- In a narrow Settings window or at 130% UI zoom, the Disconnect button and the server and telemetry address fields no longer run off the edge of their card; they wrap beneath their label.
- At small window sizes and at 130% UI zoom, the artist name, "N albums, M songs" line and Play, Shuffle and Download buttons no longer sit on top of each other (the name now clamps to two lines and the buttons drop below it), the top bar folds its controls at the right width under zoom, the full-screen player's transport stays on screen in a short window, and the sidebar, album grids, column browser and settings fields give way instead of overflowing. A layout sweep (`docs/branding/demo-library/sweep.py`) now checks every view at eight window sizes for overlap, overflow and clipping.

## [2026.10.8] - 15/10/2026

### Changed

- Settings is rebuilt on the shared settings frame, with even spacing and a wider column, regrouped into General, Playback, Library, Downloads, Services, Advanced and About. The servers now sit under Library, the scrobble service beside scrobbling under Services, and Developer is Advanced, which also holds Telemetry and the keyboard shortcuts. Connections is gone: it folded into Library and Services. A session-expiry prompt now opens Library.

### Fixed

- Saving the cache size limit, clearing the download cache, opening the data or cache folder and clearing failed operations now show an error toast when they fail, where all five used to fail silently.
- The welcome screen now says to connect a Jellyfin or OpenSubsonic server, and Open Settings lands on Library, where the servers are, instead of General.
- Switching the audio output, opening the mini player and closing it now show an error toast when they fail, and a failed output switch puts the picker back on the device that is still playing, where all three used to fail silently.
- The sync button in the top bar now shows a running sync (spinning icon, "Syncing library…") at full strength instead of greyed out, and a sync you start from it ends with a toast: "Library synced" with the counts, or the error.
- Album and artist art now refreshes when the cover changes on the server, where Bragi used to keep showing the picture it first saved.
- The artist backdrop fades out softly again instead of ending in a hard edge, in both the grid and list artist views.
- In a narrow window the now-playing panel stays centred in the top bar, instead of sliding left when the toolbar on the right needs more than its share.

## [2026.10.7] - 15/10/2026

### Fixed

- Bragi no longer loses the Jellyfin server for good after the Mac sleeps (every request failing with "dns error: no connections available" until a restart). It had its own in-process DNS resolver, which never followed the Mac's network changes; it now uses the system resolver.
- On a Mac, the close, minimise and zoom buttons stay put while you resize the window, in the main window and the MiniPlayer, instead of jumping to the top of the window until you let go. The top bar is now 52px tall, the height macOS gives its own toolbars, so macOS centres the buttons in it itself.
- Below 880px wide the window buttons no longer sit on top of the play controls: the play controls now stay clear of the window buttons at every width.
- The now-playing panel is centred on the window again at every width where it fits beside the play controls (680px and up), and narrows rather than touch them or the toolbar. A narrow panel drops its shuffle, repeat and time columns to give the title room, and below 640px wide the toolbar folds sync, lyrics and Up Next into the "more" menu as well.

## [2026.10.6] - 12/10/2026

### Fixed

- The now-playing panel no longer overlaps the toolbar: the search icon and the icons beside it keep their full size and spacing at every window width, and a long track title is cut short instead. Below 880px wide the toolbar folds its lesser icons into the "more" menu (it used to be 640px).

## [2026.10.5] - 12/10/2026

### Fixed

- A scrobble service that is throttling or timing out (429, 408) no longer has every queued play re-sent one at a time and charged an attempt: the drain stops, as it does for a server error, and the plays stay queued.
- Switching on Offline Mode stops a running server check at once rather than after its remaining retries.
- Against a plain ListenBrainz or Maloja service, a refused play is re-sent in one request, not two.

## [2026.10.4] - 12/10/2026

### Fixed

- "Can't reach your server" no longer appears when the server is up: the app declares it unreachable only after three failed probes five seconds apart, so a probe fired at launch or on wake, before the network is back, is not believed.
- A server out of reach no longer empties the library lists. Only your own Offline Mode hides what is not downloaded; an album that is cached but not downloaded used to read "No songs found".
- Plays with no MusicBrainz recording id (live recordings, bootlegs) are sent to the scrobble service by artist and title instead of being held back for ever, and the "unresolved" chip and dialog are gone. A skip with no recording id, which the scrobble server cannot record, is dropped rather than left in the pending count.
- One play the scrobble service refuses no longer blocks the rest of the queue: identified plays go first, a refused chunk is re-sent a play at a time, and plays already accepted are marked sent even when a later chunk fails.
- A server that does not answer is reported within about half a minute rather than minutes, and switching on Offline Mode while that check runs is respected.
- Search and track rows no longer treat an unreachable server as Offline Mode: cached results stay usable and a live search is still tried.

## [2026.10.3] - 09/10/2026

### Fixed

- Held listens retry on a 60-second background drain, so a listen waiting on a track's MusicBrainz ID no longer sits unsent until the next play or a manual sync.

### Internal

- Scrobble queue counts (submitted, held, pending) are exported as one structured event on the `bragi::scrobble` target.

## [2026.10.2] - 08/10/2026

### Added

- Drag the sidebar's right edge to resize it (200–420 px). The width is remembered across restarts; double-click or Home resets it, End takes the maximum, and the arrow keys nudge it.

### Internal

- Update `@poodle64/ui` to 2026.10.9.

## [2026.10.1] - 08/10/2026

### Fixed

- Recover queued listens after Jellyfin replaces track IDs, while retaining ambiguous or unverified matches for later resolution.
- Refresh the playlist sidebar after library sync, without restarting the app.
- Back and Escape dismiss search without losing the underlying artist; section changes clear search.
- Retain artist scroll position while content loads and when returning from search.
- Restore artwork when reused UI elements receive a new image.
- Extend artist backdrops across the visible panel without cropping faces; keep artists without artwork readable.
- Migrate retired telemetry helper settings to the installed collector helper, preserving configured telemetry without storing credentials.

### Internal

- Sign release bundles with the host's declared certificate and refuse ad-hoc macOS releases.
- Expose Nix packages only on Linux; macOS uses signed DMG installers and retains its development shell.
- Update the `devalue` override, test teardown, and Claude Code skill links.

## [2026.10.0] - 2026-10-05

Thirteen polish fixes from the clean-eyes pass.

### Changed

- **Sorting** — one sort order everywhere: names ignore case and accents, numbers inside titles sort as numbers, and artists sort by their proper sort name.
- **Scrolling** — Songs, playlists, the column browser and the grids all scroll through one smooth engine, and every list remembers where you were.
- **Zoom** — one zoom setting for the interface and one size control per grid, the same in every view.
- **Settings** — changes apply the moment you make them; there is no Save step, and a change the app refuses is put back.
- **Keyboard and selection** — Space plays and pauses, ⌘/Ctrl+F searches, arrow keys move through lists and album grids, and ⌘/⇧-click selects several; deleting from a playlist asks first.
- **Columns** — each view, and each playlist, keeps its own columns, order and widths; double-click a column edge to fit it to its contents.
- **Window** — full screen and window changes follow one path, and a second monitor no longer confuses it.
- **Playback and Up Next** — a new Up Next panel, Play Next and Add to Queue on tracks, and a stuck audio engine recovers instead of freezing playback.
- **Search** — one ranked search for cached and live results, and a slow answer never overwrites a newer one.
- **Playlists** — rename Jellyfin and Subsonic playlists, and a failed add now says so.
- **Sync and downloads** — downloaded badges, sync progress and offline state all come from one source, and a download you asked for is never silently removed.
- **Menus** — the same right-click menu and drag behaviour for a song, album, artist or playlist wherever it appears.
- **Look and feel** — Bragi follows the system light/dark appearance by default, and selection and now-playing rows use your accent colour.

### Fixed

- **The Songs list flashed as it scrolled** — each row took its background from its position among the rows on screen, so every scroll step flipped them all. Rows now keep their stripe by their place in the list.
- **The Songs columns drifted from their headers** — header and body are one table now, so columns line up at any window size.
- **A slow Jellyfin request froze the app** — nine commands held the server connection while waiting on the network, blocking reconnects and server switches. They let go first now, and a result that arrives after you switch servers no longer starts playback.
- **Scrolling the Songs list redrew more than once a frame** — it now redraws once per frame, like the album and artist grids.
- **Linux: Bragi crashed at launch without a tray library** — it now starts without a tray icon, and the `.deb` declares the library.

## [2026.9.1] - 2026-09-07

### Added

- **Telemetry has its own Settings section** — Bragi could only be pointed at a collector by the machine's environment, and an app launched from the Dock inherits none of it, so on any Mac that isn't set up for it Bragi stayed local with no way to change that. Settings → Connections now carries **Telemetry**: the collector's address, the command that prints its authorisation headers, a **Test** button that says whether the collector answered (or why it didn't), and **Save**, which repoints the running app without a restart. The bearer token never enters Bragi — only the command that prints it is stored, and nothing it prints is ever shown, saved or logged. Where the machine's environment already names a collector, that still wins and the fields show it read-only. What may leave the device is unchanged and still fixed in code: the four security events and the HTTP client spans, never a track title, artist, search term or file path.

## [2026.9.0] - 2026-09-07

### Fixed

- **Multi-value genre tags were stored as one string** — the Genres view listed whole tag strings as genres, so `Death Metal` and `Death Metal;Rock` were separate, non-overlapping buckets and clicking `Heavy Metal` missed every track tagged with it inside a combined string. Jellyfin does not split `;` unless the library's custom tag delimiters are configured, and Bragi then kept only the first element — lossy in both configurations. Genres are now split, trimmed, case-folded and deduped at ingest into a `track_genres` join table, and every listing, filter and AI-playlist catalogue reads that. `,` and `/` are deliberately not split, so `Folk, World, & Country` survives intact.
- **The column browser did not follow the current view** — it tracked album clicks from the Albums grid and nothing else, so opening an artist, an album from search results, or the Recently Added grid left it pinned on All / All / All while the track list showed one album. Every navigation entry point now goes through one helper, and the back/forward history records the whole selection rather than just an album id, so a genre drill-down survives it. Adds **Show in Library** (track context menu, Cmd/Ctrl+L for the now-playing track), which selects the track's genre, artist and album and scrolls the list to it.
- **The focus ring ignored the accent colour** — every focus ring stayed blue whatever accent was configured. `--p11-border-focus` is its own token, separate from `--p11-accent`, and nothing was writing it.
- **Genre cards overflowed and crowded** — long names blew out their row's height and spilled past the card border. Cards now have a fixed row height, clipped overflow, a two-line clamp with the full name on hover, and counts pinned to a common baseline.
- **A failed scrobble sync hid its own cause** — the log recorded only `error sending request for url (…)`, naming neither the DNS, TLS nor connect fault underneath it, so a transport failure could not be diagnosed from the log alone. The scrobble itself was never at risk: it stays in the local queue and drains on the next play. Every `reqwest` error the music-service client reports now carries its full source chain, so the same line reads `error sending request for url (…): client error (Connect): tcp connect error: Connection refused (os error 61)`.

### Added

- **Redesigned Genres view** — a grid of square tiles carrying a 2×2 mosaic of album covers from that genre (deterministic, drawn from the local artwork cache so it works offline; a hash-coloured tile where fewer than four covers exist), with hover-to-shuffle genre radio; or a sortable list — Genre, Songs, Albums, Artists, Total time, Years. The view choice and sort persist. Counts come from SQL rather than a walk over every cached track.

### Changed

- **Telemetry reaches your own stack, and the rolling audit file is gone** — `src-tauri/src/audit.rs` wrote a daily-rotating JSON file under the app data `logs/` directory; that file, the directory, and the Developer settings row that opened it are deleted. The subscriber is one call to the shared `poodle64/telemetry-rs` crate in the Tauri setup hook: stderr for a developer at a terminal and, where the fleet has set `OTEL_EXPORTER_OTLP_ENDPOINT`, OTLP/HTTP logs and traces to your telemetry endpoint. `bragi::audit` is the entire allow-list, so what leaves the device is the four security events — carrying a bare `scheme://host[:port]` endpoint and an error class rather than an error message, which reqwest renders with the full request URL and its query string — plus the HTTP client spans, which carry a method, host, port and status. No track title, artist, album, search term, playlist name or file path does, and a machine with none of the OTLP variables set is stderr and nothing else. Jellyfin, Subsonic and the scrobble server API calls carry W3C traceparent, so a library sync arrives in Tempo as one trace.
- **Design tokens are the single design language underneath** — every colour in the app now comes from a `--p11-*` or `--ds-color-*` token; 129 raw colour literals across 26 components are gone and stylelint refuses a new one outside the two palette files. Two new CI gates catch what no other gate can see: hand-rolling a component `@poodle64/ui` already ships, and craft defects that compile and type-check but are still wrong on screen. `EmptyState`, `ErrorState` and `LoadingState` were local copies and now come from the package, gaining the a11y roles the copies had dropped. The iTunes look is unchanged — this is the layer beneath it.
- **Build toolchain moved to Vite 8 and symphonia 0.6** — Vite 8 swaps Rollup and esbuild for Rolldown and Oxc, pulled in because `@sveltejs/vite-plugin-svelte` 7 accepts no Vite below 8; nothing in `vite.config.ts` needed porting, since it sets no bundler options. symphonia 0.6 reworked its decoding API, so the waveform decoder was rewritten against it — the RMS envelope is computed identically, so cached waveforms stay valid. rusqlite moved 0.32 → 0.40 and md5 0.7 → 0.8 without source changes. The pnpm `packageManager` pin deliberately does NOT float: it tracks the pnpm nixpkgs ships, because the hermetic Nix build installs with it offline, and Renovate is now configured to leave it alone.

### Note

Existing libraries pick up genre memberships on first launch (schema v16, applied automatically), back-derived from the tags already cached — no re-sync needed. Measured at 67 ms for 7,899 memberships across 4,044 tracks.

One case the back-derivation cannot fix on its own: if you had already enabled Jellyfin's **custom tag delimiters**, your server was returning several genres per track and Bragi kept only the first, so genres 2..n were never cached and there is nothing to derive them from. Those come back on the next library sync.

## [2026.8.4] - 2026-08-18

### Fixed

- **Recently Added was slow and showed the whole library** — opening the view took ~7.5 seconds and returned every album, ordered alphabetically. Two causes. `ANALYZE` had never run, so with no query planner statistics SQLite drove the album↔track join off a two-value index instead of the purpose-built one; `PRAGMA optimize` at database open takes that from 7,533 ms to 6 ms, and the same fix speeds up startup via `get_all_albums`. Separately, the view filtered on `last_synced_at`, which every sync rewrites library-wide with one shared timestamp — so it could only ever return everything or nothing. Albums now carry the media server's own date-added (Jellyfin `DateCreated`, OpenSubsonic `created`), which was already being requested but silently discarded. On a real 2,020-album library the view now returns 41 albums in 0.3 ms.
- **Toggle switches ignored the accent colour** — switches, buttons, checkboxes, progress bars, tabs and every focus ring stayed the default blue whatever accent was selected. The accent was being applied to the bare shadcn variables, but the Tailwind colour utilities compile against the `--ds-color-*` layer beneath them, so `bg-primary` resolved straight past the override.

### Added

- **Sort options in Recently Added** — the view had no sort control at all; the toolbar dropdown was hidden rather than empty. Adds date added (newest/oldest), year (newest/oldest) and name, with date added as the default.

### Note

Existing libraries pick up the new date-added column on first launch (schema v15, applied automatically) and backfill it on the next sync. Until that sync runs, Recently Added falls back to the previous behaviour rather than emptying out.

## [2026.8.3] - 2026-08-11

### Added

- **Shared/imported playlists** — export a playlist as a portable, MusicBrainz-recording-ID-keyed code and import it into another Bragi instance's own library, read-only and local-only (never round-trips through either side's Jellyfin). Also supports importing directly from a plain Jellyfin item link when the sender doesn't use Bragi at all, via a one-shot authenticated fetch to their server — credentials are never persisted and the session is explicitly logged out afterward.

### Changed

- **Search performance** — search now queries the local FTS5 cache instead of hitting Jellyfin live on every keystroke (previously up to ~50 sequential network round trips per search). Falls back to a live search only while a backend's first sync is still filling the cache, or if the cache unexpectedly comes back empty.
- **Full-screen lyrics and playlist polish** — long lyric lines now wrap instead of overflowing at the screen edge, initial scroll position no longer sits low before evening out, and the playlist view's now-playing highlight gets the same eq-bar animation as every other track list.

## [2026.8.2] - 2026-08-02

### Added

- **Full system Now Playing integration on macOS** — Control Center, the menu-bar widget, the lock screen and AirPods now show Bragi's title/artist/album and album artwork via `MPNowPlayingInfoCenter`, with a live scrubber and remote play/pause/seek/shuffle/repeat commands routed through `MPRemoteCommandCenter`.

### Changed

- **CI job timeouts** — every workflow job now has a bounded timeout, with alerting on cancelled runs, so a stuck job can no longer hold a runner indefinitely.
- **Hermetic Nix build** — the private `@poodle64/*` packages are vendored as tarballs under `vendor/`, so `pnpm install`, CI, and the flake build no longer need a `read:packages` token.

## [2026.8.1] - 2026-08-01

### Security

- **Patched npm-audit advisories in the dev/build toolchain** — forced patched versions of `postcss` (≥8.5.18, sourcemap path traversal), `fast-uri` (≥3.1.4, host confusion) and `cookie` (≥0.7.0) via pnpm overrides (caret-pinned to stay within each compatible major). All were transitive dev/build-only dependencies (via stylelint and SvelteKit); none ship in the app runtime, so end-user behaviour is unchanged.

## [2026.8.0] - 2026-08-01

### Added

- **Playlist collage artwork on cold launch** — sidebar playlist collages now appear on a normal launch, not only after a full library sync: a bounded, offline-aware startup prefetch warms every credentialed backend's playlist cache regardless of whether a sync was due.
- **Manual lyrics timing offset** — a global, signed, persisted correction nudges synced lyrics earlier or later (250 ms per tap) via a floating +/- stepper in the full-screen player, with click-to-reset. Local-only, never written to Jellyfin.
- **Playlist Shuffle button** — the playlist header gains a Shuffle button that shuffle-plays the whole playlist, and its Play / Shuffle / Delete buttons now share the artist header's pill-style button language.
- **Held scrobbles for un-tagged tracks** — listens and skips for tracks with no MusicBrainz recording ID are now held locally (never faked, never lost), surfaced in a warning chip and worklist dialog, and re-resolved live from Jellyfin once the track has been tagged.

### Fixed

- **Shuffle was non-functional app-wide** — the status-bar toggle only flipped a cosmetic flag; shuffle is now wired end-to-end (one-time permutation, current track pinned, restorable on toggle-off) so it actually reorders the queue everywhere.
- **Repeat toggle was dead wiring** — repeat now drives the audio engine end-to-end and the persisted mode survives a restart, mirroring the shuffle fix.
- **Now-playing row not highlighted in playlists** — the currently-playing track is highlighted in the playlist track table.
- **Full-screen repeat tooltip was static** — it now reflects the active mode (off / all / one) instead of always reading "Repeat".
- **Faster playlist first-open** — `get_playlist_tracks` no longer requests unused Jellyfin fields.
- **Scrobble sync count double-counted duplicates** — the reported synced total now uses the scrobble server's `synced` figure directly (duplicates are an informational subset); persistence was unaffected.

### Changed

- **Design-system alignment (WP-126)** — Bragi now consumes the shared `@poodle64/design-tokens` and `@poodle64/ui` packages instead of a hand-vendored shadcn-svelte set. Same visual identity (Bragi Blue, near-black dark theme); no behaviour change.

## [2026.7.7] - 2026-07-10

### Added

- **OpenSubsonic Phase 2** — playlists now work on an OpenSubsonic backend (create, edit, reorder, delete), play counts sync from both backends with a source setting, cover art is served through the authenticated local proxy, downloads get correct file extensions, each backend keeps its own background-sync schedule, and the proxy is hardened with a per-session token.
- **Nix packaging for macOS** — bragi builds as a Darwin package from the flake, installable with Nix.
- **Menu-bar rune** — the menu bar now shows the Tonlist rune as a proper macOS template icon (system-tinted for light/dark menu bars). The in-app placeholder artwork (LCD idle mark, full-screen player fallback) uses a matching gold rune instead of the retired "B" placeholder, and the stale February brand assets are gone.
- **Equaliser profiles** — dB unit labels on the sliders and saveable named custom profiles, with an overwrite warning.
- **Playlist collage artwork** — playlists without custom art show a Jellyfin-style four-album collage instead of a static music-note placeholder.

### Fixed

- **Menu-bar icon rendered as a blank square** — the full-colour app icon was flagged as a macOS template image, which flattens an opaque tile to a solid blob.
- **Wayland rendering on Linux** — force `GDK_BACKEND=x11` before GTK init to escape the broken WebKitGTK fractional-scaling path; comma-separated values like `wayland,x11` are detected too.
- **New OpenSubsonic backend's first sync was deferred** — a freshly-added second backend no longer inherits Jellyfin's legacy sync timestamp; it syncs immediately on connect.
- **AI Playlist button hidden on macOS** — a Keychain stall no longer stops the button appearing after enabling the setting.
- **Player bar controls overlapped at narrow window widths**.
- **Artist/album grids froze on fast scroll** — uniform card heights and scroll-handling fixes.
- **Equaliser 10-to-18 band mapping** — interpolated instead of sparse, with aligned frequency tables and a wider clamp.

### Changed

- **CI** — a Linux `cargo check` now gates every push and PR, so the Linux-only code path can no longer break undetected between releases; release installers build on self-hosted runners (macOS installer publishing now runs inside the flake devShell so `gh` is on the runner's PATH); the Linux AppImage is excluded from release builds.
- **macOS credentials moved out of the Keychain** — release builds now store credentials in a 0600 file in the app data dir, ending the keychain-password prompt on every rebuild (a self-signed signing identity can never satisfy the Keychain's partition-list check, which accepts only Apple-issued team IDs or per-build hashes). Linux and Windows keep the native keyring. Existing macOS users re-enter their server credentials once.

### Security

- Bumped transitive `crossbeam-epoch` 0.9.18 → 0.9.20 (RUSTSEC-2026-0204, invalid pointer dereference in a formatting impl; not on any code path bragi exercises).

## [2026.7.6] - 2026-07-06

### Added

- **OpenSubsonic support, dual-connected** — Bragi now connects to an OpenSubsonic-compatible server (Navidrome, Gonic, Airsonic) to browse, search, view lyrics, and play, **alongside** Jellyfin. Both servers can be connected at once: each keeps its own credentials and its own cached library, tagged by source so the two never mix. An **Active server** toggle (Settings → Connections) switches which library is shown and played — the switch is **instant and never re-syncs**, because both libraries stay cached side-by-side. Each backend has its own Connect/Disconnect card with an "Active" badge; disconnecting the live backend promotes the other if it's still connected. Writes are pinned to the backend being synced so a mid-sync toggle can never lose the other backend's cached library, and disconnecting a backend keeps its cache and downloads. Pick a backend under Settings → Connections → Server type.

### Fixed

- **Track search returned misaligned fields** — the full-text track search read a column list that omitted `artist_mbid`/`release_mbid`, so results had the wrong name/artist and errored on album-less tracks; the query now matches the row mapper.
- **Graceful OpenSubsonic UX** — connecting or switching to a backend whose cache is still empty now shows a "Setting up your library…" state during the first sync instead of a blank "empty library" screen; playlists (a Jellyfin-only feature) show a gentle "only available with Jellyfin" note and hide their controls when OpenSubsonic is active, instead of surfacing a raw "not supported" error.

## [2026.7.5] - 2026-07-06

### Added

- **Waveform visualisation** — the full-screen now-playing player shows a real, seekable waveform of the current track in place of the plain progress bar (played portion in the accent colour). Because mpv plays audio in a subprocess and exposes no live samples, peaks are precomputed by decoding the audio once — a locally downloaded file is streamed and decoded directly (offline-friendly), otherwise a small server-transcoded MP3 is fetched — then cached to disk keyed by track. The next queued track's waveform is prefetched in the background so advancing paints instantly. Tracks that can't be decoded (or are very long) fall back cleanly to the seek bar.

### Changed

- **Library sync no longer hammers Jellyfin or starves playback** — a manual sync is now single-flighted: triggering it while one is already running (manual + scheduled, or a reconnect) is coalesced into a no-op instead of a second concurrent full scan. Steady fetch and artwork-warming concurrency is lowered (peak connections ~14 → ~7) so a running sync no longer delays starting playback, and album fetches retry transient 5xx/429/timeout errors with backoff. Trades a slightly longer sync for a server that stays responsive to playback.

### Security

- **reqwest 0.12 → 0.13** — clears **RUSTSEC-2026-0119** (hickory-proto O(n²) name-compression CPU exhaustion) by resolving to the patched hickory-proto 0.26.1; the dated audit-ignore is removed. TLS continues to verify against the OS trust store (via rustls-platform-verifier), so self-hosted servers with a private CA in the OS store are unaffected.

## [2026.7.4] - 2026-07-05

### Added

- **AI Playlists** — generate a playlist from a natural-language prompt using an LLM of your choice: **Ollama**, any **OpenAI-compatible** endpoint, or **Anthropic**. Configure it under Settings → AI Playlists: pick a provider, **Test Connection fetches the available models** into a dropdown, and your API key is stored in the system keyring (never exposed to the UI). Generation understands the request — it extracts a structured query (artists, genres, era, "most played", count) and runs it against your library, so "Trivium's top 10 most played" returns Trivium's tracks ordered by play count, not a random sample. Play counts are tiered and configurable in Settings → General (Jellyfin → the scrobble server → none). Only track metadata is ever sent to the provider — never audio — and results are a reviewable draft you save as an ordinary Jellyfin playlist. Includes an optional custom curation prompt.
- **MiniPlayer** — a compact, always-on-top second window with album art, transport, seek, scroll-to-adjust volume, and a pin toggle. Open it from the toolbar or `Ctrl/Cmd+Shift+M`; it replaces the main window and restores it on close. Product docs added. On tiling window managers, a one-line compositor float rule is documented (Hyprland/Sway/yabai/komorebi).
- **Drag-to-reorder playlist tracks** — reorder tracks within a Jellyfin playlist by dragging (grip handle + accent drop indicator), with an optimistic update that persists to Jellyfin and reverts on failure.

## [2026.7.3] - 2026-07-05

### Added

- **Lyrics display**: a lyrics panel sourced from Jellyfin's built-in lyrics, with the currently-sung line highlighted in the active theme's accent colour. Synced lyrics track playback position; unsynced lyrics follow a progress estimate. Loading is fully decoupled from playback — lyrics lazy-load, fail gracefully, and retry with backoff without ever interrupting audio.
- **Full-screen now-playing player**: click the now-playing artwork (with a hover maximise affordance) for an Apple-Music-style full-screen view — album art and transport on the left, flowing lyrics on the right, over a blurred-glass backdrop. The active line anchors in the upper third and carousels as the song plays.
- **Download transcoding**: choose a download format (original, FLAC, MP3, AAC, Opus) and bitrate (128/192/256/320 kbps for lossy) for offline downloads; `original` keeps the exact source file.
- **Fade on pause/stop**: a 400 ms fade-out on pause/stop with a matching fade-in on resume, on by default and configurable in Playback settings.
- **Startup behaviour**: launch Bragi on system login and/or start hidden to the tray, in General settings.
- **System tray behaviour**: close-to-tray and minimise-to-tray toggles; changes take effect without a restart (macOS honours its platform conventions).
- **Session-expiry reconnect prompt**: an expired Jellyfin session now surfaces a single deduped reconnect prompt instead of failing silently.

### Fixed

- **mpv no longer outlives Bragi**: the audio subprocess is killed when Bragi exits (Linux `PR_SET_PDEATHSIG`), and any orphaned mpv instances are reaped on startup — fixing the accumulation of stray processes across hot-reloads and hard kills.
- **Scrobble crediting**: a track credits the cumulative time actually listened rather than its full duration, so partial listens scrobble accurately.

### Changed

- **Jellyfin playlist details persist locally** for instant, offline reopen — the first click into a playlist no longer waits on the network.
- **Artwork warming is detached from the sync critical path**, so a manual library sync no longer blocks on (or hammers the API for) artwork.

## [2026.7.1] - 2026-07-04

### Added

- **Offline and total-outage E2E suites**: a downloaded track plays from its local file in offline mode with the scrobble surviving; with both servers unreachable the cached library still renders, a failed sync erodes nothing, local playback works, and scrobbles stay queued.
- **Window-matrix and large-library E2E coverage**: three window sizes, virtualised scrolling across the full Songs view, and the complete playlist lifecycle (create, add, remove, reorder, delete), all asserting zero uncaught console errors.

### Fixed

- **Library cache erosion**: a sync with failed album fetches (Jellyfin 503 bursts) deleted the unfetched albums' tracks as "stale" — one such burst removed 311 live tracks. Any fetch failure now defers stale cleanup to the next fully-successful sync.
- **Cache eviction safety**: eviction and clear-all skip the currently playing track and the gapless lookahead window, and only count files actually deleted, so the cache ledger and the disk never disagree.
- **Store reactivity on large libraries**: the library and downloads stores mutate state in place instead of rebuilding whole arrays per download-progress event (the documented freeze-causing class); offline-filter caches rekeyed onto the library version counter.
- **Offline mode now gates the background sync**; a paused download manager with an emptied queue no longer reports downloading forever; the stream proxy no longer cuts off long lossless streams at 120 seconds; artwork existence checks no longer block executor threads.

## [2026.7.0] - 2026-07-04

### Added

- **E2E verification harness**: WebdriverIO + tauri-driver suite driving the built app over its real IPC surface — playback transport, an N-track soak run, stream-failure resilience, scrobble landing in the scrobble server, and a user-level audit that exercises every screen and control asserting zero uncaught console errors (WP-39).
- **Engine regression tests**: Rust tests pinning the playback fixes against a fake mpv socket.
- **Create Playlist dialogue**: the sidebar's create-playlist buttons now open a dialogue backed by the existing Jellyfin playlist capability (previously silent no-ops).
- **Genres navigation**: the Genres screen returns to the sidebar (it had become unreachable).

### Fixed

- **The random-track-skip class**: stale `idle-active` events no longer advance the queue; playlist-position changes credit the finished track by entry id (no double or missed scrobble credits); a stream-open failure retries once, then advances, then stops with an error instead of silently clearing playback; seeking near a track's end no longer stalls the transition; a crashed mpv resumes the current track instead of skipping.
- **Scrobble queue draining**: server-side duplicates now count as drained (previously they re-sent forever and were never marked synced); a successful sync marks the whole accepted batch rather than a positional subset; a service without skip support no longer aborts the drain (404/405 on `/client/skips` is handled, permanently rejected skips are dropped with a warning instead of wedging the queue).
- **mpv no longer outlives the app**: quitting Bragi shuts the audio engine down explicitly (Tauri exits without unwinding managed state, so the mpv subprocess previously kept playing after quit); the E2E harness reaps its own SIGKILL-orphaned mpv instances.
- **bits-ui data-state variants** converted to bracket form for 2.16.4.
- **`cargo test` restored in CI**; Jellyfin client tests decoupled from the credential store.

### Changed

- **Scrobbling is queue-first**: every listen queues in SQLite and drains in bulk via the scrobble server's native `POST /client/sync` (batches of 50); ListenBrainz `/1/submit-listens` remains as the fallback for other ListenBrainz-compatible services.
- **Product docs reconciled** with the mpv/localhost-proxy architecture and the queue-first scrobble pipeline; new E2E harness documentation (`e2e/README.md`, `docs/development/guide-e2e-testing.md`).

## [2026.4.0] - 2026-04-10

### Added

- **Scrobble-server track stats in library view**: play count, skip count, and last played columns in the Songs view, populated from the scrobble server's bulk stats endpoint. Columns hidden by default; opt in via right-click header menu.
- **Relative timestamp formatting**: Last Played column shows "3h ago", "2d ago" for recent plays; absolute date for older.
- **Optimistic stat updates**: play and skip counts increment immediately after scrobble/skip, before the next full refresh from the scrobble server.
- **Track stats SQLite cache**: stats persist locally for offline access (schema v5 to v6 migration).
- **Artwork caching**: album artwork cached locally for offline display.
- **SQLite-backed library queries**: column browser filtering and sorting done in SQLite via `query_library_view` command.
- **Virtual scroll improvements**: fixed jank and column resize behaviour in track list.
- **Last Played and Plays columns**: added to track list column definitions with sort support.
- **Linux support**: window controls, platform-specific CSS, Ctrl+Q shortcut.
- **Window state persistence**: remembers position and size across restarts.
- **Sync library button**: manual library refresh from toolbar.

### Fixed

- **MBID source-of-truth (critical)**: Bragi was reading Jellyfin's `MusicBrainzTrack` (release-track ID, changes per pressing) instead of `MusicBrainzRecording` (stable recording ID). Zero of Bragi's submitted MBIDs matched the actual files. After fix, 92% of library tracks have correct recording MBIDs and bulk stats match rate jumped from 1.4% to 80.5%.
- **Scrobble timestamps**: offline-queued scrobbles now carry the original play time, not the sync/submission time. Gapless playback batches get distinct timestamps reconstructed from track durations.
- **Scrobble-server API prefix**: migrated client endpoints from `/api/*` to `/client/*` per the scrobble server's API (sync, skips, bulk stats).
- **Double scrobbling**: deduplicated TrackFinished emissions during gapless playback.
- **Audio device handling**: CoreAudio device filtering, mpv device name mapping, debounced device watcher, error cascade prevention on device switch.
- **Column browser reactivity**: converted to direct property mutation to fix Svelte 5 fine-grained reactivity.
- **Frontend init ordering**: prevented reload bugs and redundant Jellyfin library syncs.

### Changed

- **Scrobble-server playlist code removed** (~2,400 lines): the scrobble server no longer has playlist endpoints. Playlists are managed exclusively via Jellyfin's native API with bidirectional sync. All scrobble-server playlist commands, HTTP client, models, stores, and UI components deleted.
- Removed dead `list_devices` example (orphaned since mpv migration).
- Removed stats feature from Bragi (belongs in the scrobble server's own dashboard).

### Documentation

- Clarified MusicBrainz recording vs release-track ID distinction across P05, P08, P09, F05 scrobbling docs, and CLAUDE.md.
- Updated P08 architecture diagram with the scrobble server's `/client/*` endpoint layout.
- Updated P05 Principle 10 to reflect stats in library view (no longer dashboard-only).
- Added regression tests for MBID field selection (`provider_ids_reads_recording_not_track`).

## [2026.1.0] - 2026-01-19

### Added

- Initial project scaffolding with Tauri v2 + Svelte 5 + TypeScript
- Conda environment configuration with Node.js 22 and pnpm
- Claude Code governance structure with master symlinks
