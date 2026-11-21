<script lang="ts">
  import {
    getJellyfinPlaylistState,
    loadJellyfinPlaylists,
    selectJellyfinPlaylist,
    requestPlaylistRename,
    requestPlaylistDelete,
  } from '#lib/stores/jellyfinPlaylist.svelte.js';
  import { addTracksToPlaylistWithDuplicateCheck } from '#lib/utils/addToPlaylist.svelte.js';
  import { getJellyfinState } from '#lib/stores/jellyfin.svelte.js';
  import { setCurrentSection, getUiState } from '#lib/stores/ui.svelte.js';
  import { getPreferences } from '#lib/stores/preferences.svelte.js';
  import type { JellyfinPlaylist, JellyfinPlaylistTrack, Track } from '#lib/types/models.js';
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { Spinner } from '#lib/components/common/index.js';
  import * as DropdownMenu from '@poodle64/ui/dropdown-menu';
  import ListMusic from '@lucide/svelte/icons/list-music';
  import Sparkles from '@lucide/svelte/icons/sparkles';
  import Play from '@lucide/svelte/icons/play';
  import PencilLine from '@lucide/svelte/icons/pencil-line';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';
  import { getAlbumTracks, getCachedJellyfinPlaylist, getJellyfinPlaylist } from '#lib/services/tauri.js';
  import { getArtistsTracks } from '#lib/utils/collectionTracks.js';
  import { jellyfinPlaylistTrackToTrack } from '#lib/utils/jellyfinPlaylist.js';
  import { startPlayback } from '#lib/utils/playback.js';
  import PlaylistCollage from './PlaylistCollage.svelte';

  interface Props {
    onCreateClick?: () => void;
    onAiClick?: () => void;
  }

  let { onCreateClick, onAiClick }: Props = $props();

  // Whether the AI-playlist feature is enabled (gates the Sparkles button),
  // read straight from the shared preferences store — no separate fetch, no
  // window event to keep it fresh.
  const aiEnabled = $derived(getPreferences().aiPlaylistEnabled);

  // The background playlist-cache prefetch just warmed the cache. Re-read
  // every listed playlist: rows that missed on a cold launch are re-armed and
  // now resolve to a collage, while already-resolved rows are skipped by the
  // guard.
  function handlePlaylistsWarmed(): void {
    for (const playlist of getJellyfinPlaylistState().playlists) {
      void loadMiniTracks(playlist.jellyfinId);
    }
  }

  onMount(() => {
    window.addEventListener('bragi-playlists-warmed', handlePlaylistsWarmed);
    return () => {
      window.removeEventListener('bragi-playlists-warmed', handlePlaylistsWarmed);
    };
  });

  let hasAttemptedLoad = $state(false);
  let dragOverPlaylistId = $state<string | null>(null);
  let addingToPlaylistId = $state<string | null>(null);

  // ── Row context menu: Play / Rename / Delete without first
  // opening the playlist — the reference apps' own sidebar behaviour. Rename
  // and Delete both select the playlist and hand off to PlaylistView's own
  // mechanism (the one rename-in-place field, the one delete-confirmation
  // dialog) rather than a second copy of either here.
  let menuOpenFor = $state<string | null>(null);

  async function handlePlaylistPlay(playlist: JellyfinPlaylist): Promise<void> {
    try {
      const detail = (await getJellyfinPlaylist(playlist.jellyfinId).catch(() => getCachedJellyfinPlaylist(playlist.jellyfinId)));
      const tracks = (detail?.tracks ?? []).map(jellyfinPlaylistTrackToTrack).filter((t): t is Track => t !== null);
      if (tracks.length === 0) return;
      await startPlayback(tracks, 0);
    } catch (e) {
      console.error('Failed to play playlist:', e);
      toast.error(`Couldn't play "${playlist.name}"`);
    }
  }

  function handleMenuRename(playlist: JellyfinPlaylist): void {
    selectJellyfinPlaylist(playlist.jellyfinId);
    setCurrentSection('playlists');
    requestPlaylistRename(playlist.jellyfinId);
  }

  function handleMenuDelete(playlist: JellyfinPlaylist): void {
    selectJellyfinPlaylist(playlist.jellyfinId);
    setCurrentSection('playlists');
    requestPlaylistDelete(playlist.jellyfinId);
  }

  // ── Sidebar cover collages ───────────────────────────────────────────
  // Sidebar rows only receive playlist summaries (no tracks), so source a mini
  // collage's covers by reading each playlist's tracks from the LOCAL SQLite
  // cache — existing IPC, offline-safe, no per-row network. Resolved tracks are
  // memoized here (keyed by playlist id); a present entry swaps ListMusic for the
  // collage, an absent one keeps ListMusic (unresolved / cache miss).
  let miniTracks = $state<Record<string, JellyfinPlaylistTrack[]>>({});
  // Non-reactive guard so each playlist is only cache-read once per session.
  const requestedMiniTracks = new Set<string>();

  async function loadMiniTracks(playlistId: string): Promise<void> {
    if (requestedMiniTracks.has(playlistId)) return;
    // Marked before the await to dedupe concurrent reads for the same row.
    requestedMiniTracks.add(playlistId);
    try {
      const detail = await getCachedJellyfinPlaylist(playlistId);
      if (detail) {
        miniTracks[playlistId] = detail.tracks;
      } else {
        // Cache miss — typically a cold launch before the background prefetch
        // has populated the cache. Re-arm this row so a later
        // 'bragi-playlists-warmed' signal (or opening the playlist) can resolve
        // it this session, instead of keeping ListMusic until the next launch.
        requestedMiniTracks.delete(playlistId);
      }
    } catch {
      // Local-cache read failure is non-fatal — re-arm and keep ListMusic.
      requestedMiniTracks.delete(playlistId);
    }
  }

  function handlePlaylistClick(playlist: JellyfinPlaylist): void {
    selectJellyfinPlaylist(playlist.jellyfinId);
    setCurrentSection('playlists');
  }

  function handleCreateClick(): void {
    onCreateClick?.();
  }

  function handleDragOver(event: DragEvent, playlistId: string): void {
    // Always prevent default to allow drop
    event.preventDefault();
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = 'copy';
    }
    dragOverPlaylistId = playlistId;
  }

  function handleDragLeave(event: DragEvent, playlistId: string): void {
    // Only clear if we're actually leaving the button, not just moving to a child element
    const relatedTarget = event.relatedTarget as Node | null;
    const currentTarget = event.currentTarget as HTMLElement;
    if (relatedTarget && currentTarget.contains(relatedTarget)) {
      return;
    }
    if (dragOverPlaylistId === playlistId) {
      dragOverPlaylistId = null;
    }
  }

  function handleDragEnter(event: DragEvent, playlistId: string): void {
    event.preventDefault();
    dragOverPlaylistId = playlistId;
  }

  async function handleDrop(event: DragEvent, playlist: JellyfinPlaylist): Promise<void> {
    event.preventDefault();
    event.stopPropagation();
    dragOverPlaylistId = null;

    if (!event.dataTransfer) {
      return;
    }

    const data = event.dataTransfer.getData('application/x-bragi-playlist');
    if (!data) {
      return;
    }

    try {
      const dragData = JSON.parse(data) as {
        type: 'tracks' | 'album' | 'artist';
        trackIds?: string[];
        albumId?: string;
        artistId?: string;
      };

      addingToPlaylistId = playlist.jellyfinId;
      let trackIds: string[] = [];

      if (dragData.type === 'tracks' && dragData.trackIds) {
        trackIds = dragData.trackIds;
      } else if (dragData.type === 'album' && dragData.albumId) {
        const tracks = await getAlbumTracks(dragData.albumId);
        trackIds = tracks.map((t) => t.jellyfinId);
      } else if (dragData.type === 'artist' && dragData.artistId) {
        const tracks = await getArtistsTracks([dragData.artistId]);
        trackIds = tracks.map((t) => t.jellyfinId);
      }

      if (trackIds.length > 0) {
        await addTracksToPlaylistWithDuplicateCheck(playlist, trackIds);
      }
    } catch (error) {
      toast.error(`Couldn't add to "${playlist.name}"`, { description: error instanceof Error ? error.message : String(error) });
    } finally {
      addingToPlaylistId = null;
    }
  }

  // Load playlists once a media server is connected. Playlists now work
  // on whichever backend is active (Jellyfin or OpenSubsonic), so the load is
  // gated only on the unified connected status; re-arm on disconnect so a
  // reconnect reloads.
  $effect(() => {
    const canLoad = getJellyfinState().status.connected;
    if (canLoad && !hasAttemptedLoad && !getJellyfinPlaylistState().isLoading) {
      hasAttemptedLoad = true;
      loadJellyfinPlaylists();
    }
    if (!canLoad) {
      hasAttemptedLoad = false;
    }
  });

  // Kick off a one-time cache read for each listed playlist to build its mini
  // collage (deduped via the non-reactive `requestedMiniTracks` set, so writing
  // `miniTracks` here can't re-trigger this effect).
  $effect(() => {
    for (const playlist of getJellyfinPlaylistState().playlists) {
      void loadMiniTracks(playlist.jellyfinId);
    }
  });

  // Keep the open playlist's row in sync with edits: add/remove replaces
  // currentPlaylist (re-runs this effect) while an in-place reorder mutates the
  // stored live tracks array, which the collage reads deeply. Also recovers rows
  // that missed the cache — opening a playlist finally populates its covers.
  $effect(() => {
    const current = getJellyfinPlaylistState().currentPlaylist;
    if (current) miniTracks[current.jellyfinId] = current.tracks;
  });
</script>

<div class="playlist-list">
  <div class="playlist-header">
    <span class="header-title">Playlists</span>
    {#if getJellyfinState().status.connected}
      <div class="header-actions">
        {#if aiEnabled}
          <button
            class="create-btn ai-btn"
            onclick={() => onAiClick?.()}
            aria-label="Create AI playlist"
            title="Create playlist with AI"
          >
            <Sparkles size={14} />
          </button>
        {/if}
        <button
          class="create-btn"
          onclick={handleCreateClick}
          aria-label="Create playlist"
          title="Create new playlist"
        >
          +
        </button>
      </div>
    {/if}
  </div>

  {#if !getJellyfinState().status.connected}
    <div class="empty">
      <span class="empty-text">Connect to a media server to manage playlists</span>
    </div>
  {:else if getJellyfinPlaylistState().isLoading}
    <div class="loading">
      <Spinner size={20} />
    </div>
  {:else if getJellyfinPlaylistState().error}
    <div class="empty">
      <span class="empty-text">{getJellyfinPlaylistState().error}</span>
    </div>
  {:else if getJellyfinPlaylistState().playlists.length === 0}
    <div class="empty">
      <span class="empty-text">No playlists yet</span>
      <button class="create-first-btn" onclick={handleCreateClick}>
        Create your first playlist
      </button>
    </div>
  {:else}
    <ul class="playlists">
      {#each getJellyfinPlaylistState().playlists as playlist (playlist.jellyfinId)}
        {@const rowTracks = miniTracks[playlist.jellyfinId]}
        <li class="playlist-row">
          <button
            class="playlist-item"
            class:selected={getJellyfinPlaylistState().selectedPlaylistId === playlist.jellyfinId &&
              getUiState().currentSection === 'playlists'}
            class:drag-over={dragOverPlaylistId === playlist.jellyfinId}
            class:adding={addingToPlaylistId === playlist.jellyfinId}
            onclick={() => handlePlaylistClick(playlist)}
            oncontextmenu={(e) => { e.preventDefault(); menuOpenFor = playlist.jellyfinId; }}
            ondragenter={(e) => handleDragEnter(e, playlist.jellyfinId)}
            ondragover={(e) => handleDragOver(e, playlist.jellyfinId)}
            ondragleave={(e) => handleDragLeave(e, playlist.jellyfinId)}
            ondrop={(e) => handleDrop(e, playlist)}
            title={`${playlist.name} (${playlist.itemCount} tracks)`}
          >
            <span class="playlist-icon">
              {#if addingToPlaylistId === playlist.jellyfinId}
                <Spinner size={14} />
              {:else if rowTracks}
                <PlaylistCollage tracks={rowTracks} emptyGlyphSize={16} />
              {:else}
                <ListMusic size={16} />
              {/if}
            </span>
            <span class="playlist-name">{playlist.name}</span>
          </button>
          <span class="playlist-row-menu">
            <DropdownMenu.Root
              bind:open={
                () => menuOpenFor === playlist.jellyfinId,
                (v) => { menuOpenFor = v ? playlist.jellyfinId : null; }
              }
            >
              <DropdownMenu.Trigger>
                {#snippet child({ props })}
                  <button {...props} class="playlist-menu-trigger" aria-label="Playlist actions for {playlist.name}">
                    <EllipsisVertical size={14} />
                  </button>
                {/snippet}
              </DropdownMenu.Trigger>
              <DropdownMenu.Content align="end">
                <DropdownMenu.Item onclick={() => void handlePlaylistPlay(playlist)}>
                  <Play size={14} />
                  <span>Play</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item onclick={() => handleMenuRename(playlist)}>
                  <PencilLine size={14} />
                  <span>Rename</span>
                </DropdownMenu.Item>
                <DropdownMenu.Separator />
                <DropdownMenu.Item onclick={() => handleMenuDelete(playlist)}>
                  <Trash2 size={14} />
                  <span>Delete</span>
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .playlist-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    width: 100%;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .playlist-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 var(--space-sm);
    margin-bottom: var(--space-xs);
    flex-shrink: 0;
  }

  .header-title {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .create-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-muted);
    font-size: var(--font-size-lg);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .create-btn:hover {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-accent);
  }

  .loading {
    display: flex;
    justify-content: center;
    padding: var(--space-md);
  }

  .empty {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
    padding: var(--space-sm);
    text-align: center;
  }

  .empty-text {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .create-first-btn {
    padding: var(--space-xs) var(--space-sm);
    background-color: var(--p11-accent);
    border: none;
    border-radius: var(--p11-radius-sm);
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-sm);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .create-first-btn:hover {
    background-color: var(--p11-accent-hover);
  }

  .playlists {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    overflow-x: hidden;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-height: 0;
  }

  .playlist-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    width: 100%;
    padding: var(--space-xs) var(--space-md);
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-secondary);
    font-size: var(--font-size-md);
    font-family: var(--font-family);
    text-align: left;
    cursor: pointer;
    transition: all var(--transition-fast);
    line-height: 1.4;
    min-height: 32px;
    flex: 1;
    min-width: 0;
  }

  .playlist-item:hover {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-primary);
  }

  .playlist-item.selected {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-accent);
  }

  .playlist-item.drag-over {
    background-color: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
    outline: 2px solid var(--p11-accent);
    outline-offset: -2px;
  }

  .playlist-item.adding {
    opacity: 0.7;
    pointer-events: none;
  }

  .playlist-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    /* Round + clip so the 2x2 mini-collage reads as a small cover thumbnail. */
    border-radius: var(--p11-radius-sm);
    overflow: hidden;
  }

  .playlist-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .playlist-row {
    display: flex;
    align-items: center;
  }

  .playlist-row-menu {
    flex-shrink: 0;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .playlist-row:hover .playlist-row-menu,
  .playlist-row:focus-within .playlist-row-menu,
  .playlist-row:has(.playlist-menu-trigger[data-state='open']) .playlist-row-menu {
    opacity: 1;
  }

  .playlist-menu-trigger {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-muted);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .playlist-menu-trigger:hover {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-primary);
  }
</style>
