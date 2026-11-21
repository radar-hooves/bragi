# Bragi

Desktop music application. Offline-first music player interfacing with self-hosted Jellyfin while maintaining complete data sovereignty.

## Scope

- Must work offline: browse cached library, play cached audio, queue scrobbles
- Must never lose a scrobble — persist locally first, sync when online
- Must treat Jellyfin as read-only for play state, play counts, and ratings; playlists are the deliberate exception (stored in and written to Jellyfin, bidirectionally)
- Does not embed a scrobble backend — delegates to a configurable endpoint (ListenBrainz, Maloja, or none)

## Pitfalls

- The `@poodle64/*` deps (`design-tokens`, `ui`) are public on npmjs — no `.npmrc`, no token, in `pnpm install`, CI or the hermetic Nix build. After a version bump, refresh the `pnpmDeps` hash in `flake.nix`. Re-introducing `file:` tarballs would silently freeze this app's design language
- CSP `style-src 'unsafe-inline'` in `tauri.conf.json` is required for Svelte scoped styles. Do not remove
- Telemetry: a request URL is stripped with `.without_url()` before it reaches `telemetry::report_error_with_cause`, everywhere a Jellyfin/Subsonic call can carry a query-string auth token — the one shape `report_error_with_cause` itself does not sanitise for you. A credential-bearing command (login, connect, keyring) carries name/outcome only (`traced`, never `traced_with_span`). Outbound API calls go through `src-tauri/src/http.rs`, never a bare `reqwest::Client`; the exceptions are deliberate — the streaming and download clients (`build_download_client`, Subsonic's `download_http`, `proxy.rs`) carry no total timeout, so never route a transcode onto the traced client
- Anything driving a Tailwind colour _utility_ (`bg-primary`, `ring-ring`) must override `--ds-color-*`, not the bare shadcn name: `@poodle64/design-tokens` registers `--color-primary: var(--ds-color-primary)`, so setting `--primary` alone leaves every `@poodle64/ui` primitive on the static palette in `src/app.css`
- The app process is lowercase `bragi` (`/Applications/Bragi.app/Contents/MacOS/bragi`) — `pgrep -x Bragi` matches nothing and misreports "not running"
- To verify the menu-bar tray icon: `screencapture -x -D 1 full.png`, then crop the top-right strip with `sips`; the Finder desktop-bounds trick spans displays and can capture a window instead

## Architecture notes

Bragi is a Tauri v2 desktop app with Rust IPC, not a REST API:

- Frontend calls go through Tauri `invoke()` IPC in `src/lib/services/tauri.ts`, so there is no OpenAPI client.
- Settings are key-value pairs saved via Tauri IPC, not HTML form submissions, so there is no form library.
- The sidebar is custom, with music-player navigation (library views, queue, playlists), not the stock shadcn Sidebar.
- The player shell is one route, a persistent transport bar and a three-column filter surface, built on the shared tokens and primitives rather than a web app shell.
