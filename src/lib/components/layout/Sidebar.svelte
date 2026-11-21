<script lang="ts">
  import { getUiState, setCurrentSection, toggleSidebar, type LibrarySection } from '#lib/stores/ui.svelte.js';
  import { navigateTo } from '#lib/stores/navigation.svelte.js';
  import { getPlaybackState } from '#lib/stores/player.svelte.js';
  import { getPendingDownloadCount, getFailedDownloadCount } from '#lib/stores/downloads.svelte.js';
  import { SyncStatusIndicator } from '#lib/components/common/index.js';
  import { PlaylistList, SharedPlaylistList } from '#lib/components/playlist/index.js';
  import { cachedArtworkUrl, handleArtworkError, resetArtwork } from '#lib/utils/artworkFallback.js';
  import { bindScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
  import { ResizeHandle } from '@poodle64/ui/resize-handle';
  import type { Component } from 'svelte';
  import Disc3 from '@lucide/svelte/icons/disc-3';
  import Users from '@lucide/svelte/icons/users';
  import Music from '@lucide/svelte/icons/music';
  import Layers from '@lucide/svelte/icons/layers';
  import Clock from '@lucide/svelte/icons/clock';
  import History from '@lucide/svelte/icons/history';
  import Download from '@lucide/svelte/icons/download';
  import ListMusic from '@lucide/svelte/icons/list-music';
  import Share2 from '@lucide/svelte/icons/share-2';
  import PanelLeftClose from '@lucide/svelte/icons/panel-left-close';
  import PanelLeft from '@lucide/svelte/icons/panel-left';

  interface Props {
    onCreatePlaylist?: () => void;
    onCreateAiPlaylist?: () => void;
    onImportPlaylist?: () => void;
  }

  let { onCreatePlaylist, onCreateAiPlaylist, onImportPlaylist }: Props = $props();

  let width = $state<number | null>(null);
  let dragging = $state(false);

  // .nav is the sidebar's one real scroll owner: PlaylistList/SharedPlaylistList
  // sit inside a plain (non-flex) .nav-group, so their own .playlists overflow
  // rules never actually engage here — the static nav items and both playlist
  // lists all scroll together as one region. Remembered here, not in either
  // list component.
  let navEl = $state<HTMLElement>();
  $effect(() => bindScrollMemory('sidebar-nav', navEl));

  interface NavItem {
    id: LibrarySection;
    label: string;
    icon: Component;
  }

  interface NavGroup {
    title: string;
    items: NavItem[];
  }

  const libraryGroup: NavGroup = {
    title: 'Library',
    items: [
      { id: 'albums', label: 'Albums', icon: Disc3 },
      { id: 'artists', label: 'Artists', icon: Users },
      { id: 'tracks', label: 'Songs', icon: Music },
      { id: 'genres', label: 'Genres', icon: Layers },
      { id: 'recentlyAdded', label: 'Recently Added', icon: Clock },
      { id: 'recentlyPlayed', label: 'Recently Played', icon: History },
    ],
  };

  const activityGroup: NavGroup = {
    title: 'Activity',
    items: [
      { id: 'queue', label: 'Queue', icon: Download },
    ],
  };

  const downloadBadgeCount = $derived(getPendingDownloadCount() + getFailedDownloadCount());
  const navGroups = [libraryGroup, activityGroup];

  function handleNavClick(section: LibrarySection): void {
    setCurrentSection(section);
    navigateTo({ section, selectedArtistId: null, selectedAlbumId: null });
  }

  function handleCreatePlaylistClick(): void {
    onCreatePlaylist?.();
  }

  function handleToggleSidebar(): void {
    toggleSidebar();
  }
</script>

<aside
  class="sidebar"
  class:collapsed={getUiState().sidebarCollapsed}
  class:dragging
  style:width={!getUiState().sidebarCollapsed && width !== null ? `${width}px` : undefined}
>
  <nav class="nav" bind:this={navEl}>
    {#each navGroups as group, index}
      {#if index > 0 && !getUiState().sidebarCollapsed}
        <div class="nav-separator"></div>
      {/if}
      <div class="nav-group">
        {#if !getUiState().sidebarCollapsed}
          <h2 class="nav-section-title">{group.title}</h2>
        {/if}
        <ul class="nav-list">
          {#each group.items as item}
            {@const Icon = item.icon}
            <li>
              <button
                class="nav-item"
                class:active={getUiState().currentSection === item.id}
                onclick={() => handleNavClick(item.id)}
                title={getUiState().sidebarCollapsed ? item.label : undefined}
              >
                <span class="nav-icon">
                  <Icon size={16} />
                </span>
                {#if !getUiState().sidebarCollapsed}
                  <span class="nav-label">{item.label}</span>
                  {#if item.id === 'queue' && downloadBadgeCount > 0}
                    <span class="nav-badge" class:has-failed={getFailedDownloadCount() > 0}
                      >{downloadBadgeCount}</span
                    >
                  {/if}
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/each}

    {#if !getUiState().sidebarCollapsed}
      <div class="nav-separator"></div>
      <div class="nav-group">
        <PlaylistList onCreateClick={handleCreatePlaylistClick} onAiClick={() => onCreateAiPlaylist?.()} />
      </div>
      <div class="nav-separator"></div>
      <div class="nav-group">
        <SharedPlaylistList onImportClick={() => onImportPlaylist?.()} />
      </div>
    {:else}
      <div class="nav-group">
        <ul class="nav-list">
          <li>
            <button
              class="nav-item"
              class:active={getUiState().currentSection === 'playlists'}
              onclick={() => handleNavClick('playlists')}
              title="Playlists"
            >
              <span class="nav-icon">
                <ListMusic size={16} />
              </span>
            </button>
          </li>
          <li>
            <button
              class="nav-item"
              class:active={getUiState().currentSection === 'sharedPlaylists'}
              onclick={() => handleNavClick('sharedPlaylists')}
              title="Shared playlists"
            >
              <span class="nav-icon">
                <Share2 size={16} />
              </span>
            </button>
          </li>
        </ul>
      </div>
    {/if}
  </nav>

  <div class="sidebar-bottom">
    {#if !getUiState().sidebarCollapsed}
      <div class="sidebar-controls">
        <div class="sync-indicator">
          <SyncStatusIndicator />
        </div>
        <button
          class="collapse-btn"
          onclick={handleToggleSidebar}
          aria-label="Collapse sidebar"
          title="Collapse sidebar"
        >
          <PanelLeftClose size={16} />
        </button>
      </div>
      <div class="sidebar-artwork">
        {#if getPlaybackState().currentTrack?.albumJellyfinId}
          <img
            src={cachedArtworkUrl(getPlaybackState().currentTrack!.albumJellyfinId)}
            use:resetArtwork={cachedArtworkUrl(getPlaybackState().currentTrack!.albumJellyfinId)}
            alt="Album artwork"
            class="sidebar-artwork-image"
            onerror={(e) => handleArtworkError(e, getPlaybackState().currentTrack?.artworkUrl)}
          />
          <div class="sidebar-artwork-placeholder" style="display:none">
            <Music size={48} strokeWidth={1.5} />
          </div>
        {:else}
          <div class="sidebar-artwork-placeholder">
            <Music size={48} strokeWidth={1.5} />
          </div>
        {/if}
      </div>
    {:else}
      <button
        class="collapse-btn collapsed-toggle"
        onclick={handleToggleSidebar}
        aria-label="Expand sidebar"
        title="Expand sidebar"
      >
        <PanelLeft size={16} />
      </button>
    {/if}
  </div>
  {#if !getUiState().sidebarCollapsed}
    <ResizeHandle bind:width bind:dragging storageKey="bragi-sidebar-width" />
  {/if}
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    position: relative;
    width: var(--sidebar-width);
    /* Yields to the content on a narrow window, never below the collapsed rail. */
    max-width: max(var(--sidebar-collapsed-width), 38%);
    height: 100%;
    background-color: var(--p11-bg-surface);
    backdrop-filter: blur(20px) saturate(1.4);
    -webkit-backdrop-filter: blur(20px) saturate(1.4);
    border-right: 1px solid var(--p11-border);
    transition: width var(--transition-normal);
  }

  /* Disable expensive backdrop-filter on Linux for performance */
  :global([data-platform='linux']) .sidebar {
    backdrop-filter: none;
    -webkit-backdrop-filter: none;
  }

  .sidebar.dragging {
    transition: none;
  }

  .sidebar.collapsed {
    width: var(--sidebar-collapsed-width);
  }

  .nav {
    flex: 1;
    padding: var(--space-sm) var(--space-md);
    overflow-y: auto;
  }

  .nav-group {
    margin-bottom: var(--space-sm);
  }

  .nav-group:last-child {
    margin-bottom: 0;
  }

  .nav-section-title {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 var(--space-xs) var(--space-sm);
  }

  .nav-separator {
    height: 1px;
    background-color: var(--p11-border);
    margin: var(--space-sm) 0;
  }

  .nav-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    width: 100%;
    padding: 5px var(--space-md);
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-secondary);
    font-size: var(--font-size-md);
    font-family: var(--font-family);
    text-align: left;
    cursor: pointer;
    position: relative;
    transition:
      background-color var(--transition-fast),
      color var(--transition-fast);
  }

  .nav-item:hover {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-primary);
  }

  .nav-item.active {
    background-color: var(--p11-accent-subtle);
    color: var(--p11-accent);
    font-weight: var(--font-weight-medium);
  }

  .nav-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    flex-shrink: 0;
  }

  .nav-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .nav-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    margin-left: auto;
    background-color: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-2xs);
    font-weight: var(--font-weight-bold);
    border-radius: var(--radius-full);
    line-height: 1;
  }

  .nav-badge.has-failed {
    background-color: var(--p11-error);
  }

  .collapsed .nav-item {
    justify-content: center;
    padding: var(--space-sm);
  }

  .collapsed .nav-icon {
    width: auto;
  }

  /* Bottom section: controls + artwork */
  .sidebar-bottom {
    display: flex;
    flex-direction: column;
    margin-top: auto;
    /* On a short window the artwork crops rather than push the nav out. */
    max-height: 45%;
    min-height: 0;
  }

  .sidebar-controls {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-xs) var(--space-sm) var(--space-xs) var(--space-md);
  }

  .sync-indicator {
    flex: 1;
  }

  .collapse-btn {
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
    transition:
      color var(--transition-fast),
      background-color var(--transition-fast);
  }

  .collapse-btn:hover {
    color: var(--p11-text-primary);
    background-color: var(--p11-bg-elevated);
  }

  .collapsed-toggle {
    margin: var(--space-sm) auto;
  }

  .sidebar-artwork {
    flex: 0 1 auto;
    min-height: 0;
    overflow: hidden;
    padding: 0;
  }

  .sidebar-artwork-image {
    width: 100%;
    height: 100%;
    aspect-ratio: 1;
    object-fit: cover;
    display: block;
  }

  .sidebar-artwork-placeholder {
    width: 100%;
    aspect-ratio: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-muted);
  }
</style>
