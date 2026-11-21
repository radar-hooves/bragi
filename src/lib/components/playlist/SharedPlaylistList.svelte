<script lang="ts">
  import { getImportedPlaylistState, loadImportedPlaylists, selectImportedPlaylist } from '#lib/stores/importedPlaylists.svelte.js';
  import type { ImportedPlaylist } from '#lib/types/models.js';
  import { setCurrentSection, getUiState } from '#lib/stores/ui.svelte.js';
  import { onMount } from 'svelte';
  import { Spinner } from '#lib/components/common/index.js';
  import ListMusic from '@lucide/svelte/icons/list-music';

  interface Props {
    onImportClick?: () => void;
  }

  let { onImportClick }: Props = $props();

  onMount(() => {
    void loadImportedPlaylists();
  });

  function handlePlaylistClick(playlist: ImportedPlaylist): void {
    selectImportedPlaylist(playlist.id);
    setCurrentSection('sharedPlaylists');
  }

  function handleImportClick(): void {
    onImportClick?.();
  }
</script>

<div class="shared-playlist-list">
  <div class="shared-playlist-header">
    <span class="header-title">Shared</span>
    <button class="import-btn" onclick={handleImportClick} aria-label="Import shared playlist" title="Import a shared playlist">
      +
    </button>
  </div>

  {#if getImportedPlaylistState().isLoading && getImportedPlaylistState().playlists.length === 0}
    <div class="loading">
      <Spinner size={20} />
    </div>
  {:else if getImportedPlaylistState().playlists.length === 0}
    <div class="empty">
      <span class="empty-text">No shared playlists yet</span>
    </div>
  {:else}
    <ul class="playlists">
      {#each getImportedPlaylistState().playlists as playlist (playlist.id)}
        <li>
          <button
            class="playlist-item"
            class:selected={getImportedPlaylistState().selectedPlaylistId === playlist.id &&
              getUiState().currentSection === 'sharedPlaylists'}
            onclick={() => handlePlaylistClick(playlist)}
            title={playlist.name}
          >
            <span class="playlist-icon">
              <ListMusic size={16} />
            </span>
            <span class="playlist-name">{playlist.name}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .shared-playlist-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
    width: 100%;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .shared-playlist-header {
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

  .import-btn {
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

  .import-btn:hover {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-accent);
  }

  .loading {
    display: flex;
    justify-content: center;
    padding: var(--space-md);
  }

  .empty {
    padding: var(--space-sm);
    text-align: center;
  }

  .empty-text {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
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
  }

  .playlist-item:hover {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-primary);
  }

  .playlist-item.selected {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-accent);
  }

  .playlist-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    border-radius: var(--p11-radius-sm);
    overflow: hidden;
  }

  .playlist-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }
</style>
