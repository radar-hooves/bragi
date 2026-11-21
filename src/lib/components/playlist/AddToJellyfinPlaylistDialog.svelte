<script lang="ts">
  import { onMount } from 'svelte';
  import {
    getJellyfinPlaylistState,
    loadJellyfinPlaylists,
    addTracksToJellyfinPlaylist,
    createJellyfinPlaylist,
  } from '#lib/stores/jellyfinPlaylist.svelte.js';
  import { addTracksToPlaylistWithDuplicateCheck } from '#lib/utils/addToPlaylist.svelte.js';
  import type { JellyfinPlaylist, Track } from '#lib/types/models.js';
  import { Spinner } from '#lib/components/common/index.js';
  import * as Dialog from '@poodle64/ui/dialog';
  import ListMusic from '@lucide/svelte/icons/list-music';

  interface Props {
    /** Tracks to add to a playlist. */
    tracks: Track[];
    /** Called when dialog is closed. */
    onClose: () => void;
    /** Called when tracks are successfully added. */
    onSuccess?: (playlist: JellyfinPlaylist) => void;
  }

  let { tracks, onClose, onSuccess }: Props = $props();

  let dialogOpen = $state(true);
  let selectedPlaylistId = $state<string | null>(null);
  let showCreateNew = $state(false);
  let newPlaylistName = $state('');
  let error = $state<string | null>(null);
  let isAdding = $state(false);

  // Get Jellyfin IDs from tracks
  const trackJellyfinIds = $derived(tracks.map((t) => t.jellyfinId));

  const canAdd = $derived(
    trackJellyfinIds.length > 0 &&
      !isAdding &&
      (selectedPlaylistId !== null || (showCreateNew && newPlaylistName.trim().length > 0))
  );

  $effect(() => {
    if (!dialogOpen) onClose();
  });

  onMount(() => {
    if (getJellyfinPlaylistState().playlists.length === 0 && !getJellyfinPlaylistState().isLoading) {
      loadJellyfinPlaylists();
    }
  });

  function handlePlaylistSelect(playlistId: string): void {
    selectedPlaylistId = playlistId;
    showCreateNew = false;
    error = null;
  }

  function handleCreateNewClick(): void {
    showCreateNew = true;
    selectedPlaylistId = null;
    error = null;
  }

  async function handleAdd(): Promise<void> {
    if (!canAdd) return;

    isAdding = true;
    error = null;

    try {
      let targetPlaylist: JellyfinPlaylist | null = null;
      let isNewPlaylist = false;

      if (showCreateNew && newPlaylistName.trim()) {
        // Create new playlist first
        targetPlaylist = await createJellyfinPlaylist(newPlaylistName.trim());
        isNewPlaylist = true;
        if (!targetPlaylist) {
          error = getJellyfinPlaylistState().error ?? 'Failed to create playlist';
          isAdding = false;
          return;
        }
      } else if (selectedPlaylistId !== null) {
        targetPlaylist =
          getJellyfinPlaylistState().playlists.find((p) => p.jellyfinId === selectedPlaylistId) ?? null;
      }

      if (!targetPlaylist) {
        error = 'No playlist selected';
        isAdding = false;
        return;
      }

      // A freshly-created playlist can hold no duplicates yet, so it skips
      // straight to the store action; an existing one goes through the one
      // shared duplicate-check path every add-to-playlist entry point uses.
      // That path also returns `false` when the user cancels the duplicate
      // question — not a failure, so it reports no error and just re-enables
      // the form for another try.
      if (isNewPlaylist) {
        const success = await addTracksToJellyfinPlaylist(targetPlaylist.jellyfinId, trackJellyfinIds);
        if (success) {
          onSuccess?.(targetPlaylist);
          dialogOpen = false;
        } else {
          error = getJellyfinPlaylistState().error ?? 'Failed to add tracks';
        }
      } else {
        const added = await addTracksToPlaylistWithDuplicateCheck(targetPlaylist, trackJellyfinIds);
        if (added) {
          onSuccess?.(targetPlaylist);
          dialogOpen = false;
        }
      }
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      isAdding = false;
    }
  }
</script>

<Dialog.Root bind:open={dialogOpen}>
  <Dialog.Content class="add-to-playlist-dialog">
    <Dialog.Header>
      <Dialog.Title>Add to Playlist</Dialog.Title>
    </Dialog.Header>

    <div class="dialog-body">
      <div class="tracks-info">
        {#if tracks.length === 1}
          <span class="track-count">Adding 1 track</span>
          <span class="track-name">{tracks[0]?.name}</span>
        {:else}
          <span class="track-count">Adding {tracks.length} tracks</span>
        {/if}
      </div>

      {#if getJellyfinPlaylistState().isLoading}
        <div class="loading">
          <Spinner size={32} />
          <span>Loading playlists...</span>
        </div>
      {:else if getJellyfinPlaylistState().error && getJellyfinPlaylistState().playlists.length === 0}
        <div class="error-state">
          <span>{getJellyfinPlaylistState().error}</span>
          <button class="retry-btn" onclick={loadJellyfinPlaylists}>Retry</button>
        </div>
      {:else}
        <div class="playlist-options">
          <button
            class="create-new-option"
            class:selected={showCreateNew}
            onclick={handleCreateNewClick}
          >
            <span class="option-icon">+</span>
            <span>Create new playlist</span>
          </button>

          {#if showCreateNew}
            <div class="new-playlist-input">
              <!-- svelte-ignore a11y_autofocus -->
              <input
                type="text"
                bind:value={newPlaylistName}
                placeholder="Playlist name"
                maxlength="255"
                autofocus
              />
            </div>
          {/if}

          {#if getJellyfinPlaylistState().playlists.length > 0}
            <div class="divider">or add to existing playlist</div>

            <ul class="playlist-list">
              {#each getJellyfinPlaylistState().playlists as playlist (playlist.jellyfinId)}
                <li>
                  <button
                    class="playlist-option"
                    class:selected={selectedPlaylistId === playlist.jellyfinId}
                    onclick={() => handlePlaylistSelect(playlist.jellyfinId)}
                  >
                    <span class="playlist-icon">
                      <ListMusic size={16} />
                    </span>
                    <span class="playlist-name">{playlist.name}</span>
                    <span class="playlist-count">{playlist.itemCount}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}

      {#if error}
        <div class="error-message">{error}</div>
      {/if}

      <div class="dialog-actions">
        <button class="cancel-btn" onclick={() => (dialogOpen = false)} disabled={isAdding}> Cancel </button>
        <button class="add-btn" onclick={handleAdd} disabled={!canAdd}>
          {#if isAdding}
            <Spinner size={16} />
            Adding...
          {:else}
            Add to Playlist
          {/if}
        </button>
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>

<style>
  .dialog-body {
    padding: 0 var(--space-lg) var(--space-lg);
  }

  .tracks-info {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    margin-bottom: var(--space-lg);
    padding: var(--space-md);
    background-color: var(--p11-bg-elevated);
    border-radius: var(--p11-radius-md);
  }

  .track-count {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .track-name {
    font-weight: var(--font-weight-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-xl);
    color: var(--p11-text-secondary);
  }

  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-lg);
    color: var(--p11-error);
  }

  .retry-btn {
    padding: var(--space-xs) var(--space-md);
    background-color: var(--p11-bg-elevated);
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-primary);
    cursor: pointer;
  }

  .playlist-options {
    display: flex;
    flex-direction: column;
    gap: var(--space-sm);
  }

  .create-new-option,
  .playlist-option {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    background: transparent;
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    color: var(--p11-text-primary);
    font-size: var(--font-size-md);
    text-align: left;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .create-new-option:hover,
  .playlist-option:hover {
    background-color: var(--p11-bg-elevated);
    border-color: var(--p11-accent);
  }

  .create-new-option.selected,
  .playlist-option.selected {
    background-color: var(--p11-selection);
    border-color: var(--p11-accent);
    color: var(--p11-accent);
  }

  .option-icon {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-bold);
  }

  .new-playlist-input {
    padding-left: var(--space-xl);
  }

  .new-playlist-input input {
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    color: var(--p11-text-primary);
    font-size: var(--font-size-md);
    font-family: var(--font-family);
  }

  .new-playlist-input input:focus {
    outline: none;
    border-color: var(--p11-accent);
  }

  .divider {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
    margin: var(--space-md) 0;
  }

  .divider::before,
  .divider::after {
    content: '';
    flex: 1;
    height: 1px;
    background-color: var(--p11-border);
  }

  .playlist-list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 200px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
  }

  .playlist-icon {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    color: var(--p11-text-muted);
  }

  .playlist-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playlist-count {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .error-message {
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-error-bg);
    border-radius: var(--p11-radius-md);
    color: var(--p11-error);
    font-size: var(--font-size-sm);
    margin: var(--space-lg) 0;
  }

  .dialog-actions {
    display: flex;
    gap: var(--space-md);
    justify-content: flex-end;
    margin-top: var(--space-lg);
  }

  .cancel-btn,
  .add-btn {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-lg);
    border-radius: var(--p11-radius-md);
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .cancel-btn {
    background: transparent;
    border: 1px solid var(--p11-border);
    color: var(--p11-text-primary);
  }

  .cancel-btn:hover:not(:disabled) {
    background-color: var(--p11-bg-elevated);
  }

  .add-btn {
    background-color: var(--p11-accent);
    border: none;
    color: rgb(var(--p11-overlay-rgb));
  }

  .add-btn:hover:not(:disabled) {
    background-color: var(--p11-accent-hover);
  }

  .cancel-btn:disabled,
  .add-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
