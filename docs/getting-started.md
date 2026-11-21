# Getting started

Bragi plays the library on your own music server. You need a Jellyfin server, or
an OpenSubsonic one (Navidrome, Gonic, Airsonic), and an account on it.

## 1. Connect your server

On first launch, Bragi asks you to connect a server. **Open Settings** takes you to
**Library**, where the servers are.

- **Jellyfin:** enter the server URL (`jellyfin.example.com`), your username and
  password, and choose **Sign in**.
- **OpenSubsonic:** enter the server URL, your username and password. OpenSubsonic
  signs every request with your password, so Bragi stores it on this machine.

Bragi then copies the library's details and artwork to your machine; a large
library takes a few minutes the first time. **Auto-sync** in **Settings > Library**
keeps it current, and the sync button in the toolbar syncs now.

If you have both kinds of server, connect both and pick the **Active server** in
**Settings > Library**.

## 2. Scrobble your plays (optional)

In **Settings > Services**, turn on **Enable scrobbling** and connect a
ListenBrainz-compatible service:

- **Service URL:** `https://api.listenbrainz.org` for ListenBrainz, or your own
  service's address (Maloja, or another that speaks the ListenBrainz API).
- **API token:** your ListenBrainz user token, or your service's API key.

**Scrobble threshold** sets how much of a song must play before it counts. Every
play is queued on your machine and retried until the service accepts it;
**Offline queue** shows what is still waiting.

## 3. Listen offline

- **Download:** the download button on an album or song keeps a copy on your
  machine. **Settings > Downloads** sets the format and bitrate, and the
  **Maximum cache size**; past it, the least recently used copies go first.
- **Offline Mode:** the Wi-Fi button in the toolbar takes Bragi offline on purpose.
  The whole library stays browsable and searchable; songs you have downloaded play,
  and the rest are greyed out until you go back online.

## 4. Updates

On macOS, Bragi checks for a new version when it starts and every six hours, and
offers it in a notification. **Settings > About** shows the last check, and
**Check now** checks again. An update installs and restarts Bragi when you say so.

On Linux, install each new `.deb` or `.rpm` from the
[releases page](https://github.com/radar-hooves/bragi/releases), or update the Nix
profile.

## Where Bragi keeps its data

**Settings > About** shows the data directory: the library cache, your settings and
the scrobble queue. Downloaded songs live in the cache folder that
**Settings > Downloads** names. Removing the app leaves both; delete them to start
again.
