<script lang="ts">
  import { onMount } from 'svelte';
  import { getUiState, getUpNextVisible, toggleUpNext, setCurrentSection } from '#lib/stores/ui.svelte.js';
  import { getOfflineMode, setOfflineMode } from '#lib/stores/offlineMode.svelte.js';
  import { getIsSyncing } from '#lib/stores/syncStatus.svelte.js';
  import { toast } from 'svelte-sonner';
  import { getPlaybackState } from '#lib/stores/player.svelte.js';
  import { getLyricsVisible, toggleLyrics } from '#lib/stores/lyrics.svelte.js';
  import { SearchBar } from '#lib/components/library/index.js';
  import { getAudioDevices, setAudioDevices } from '#lib/stores/audioDevices.svelte.js';
  import { getOutputDevices, setOutputDevice } from '#lib/services/tauri.js';
  import { navigateTo } from '#lib/stores/navigation.svelte.js';
  import Settings from '@lucide/svelte/icons/settings';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Wifi from '@lucide/svelte/icons/wifi';
  import WifiOff from '@lucide/svelte/icons/wifi-off';
  import Speaker from '@lucide/svelte/icons/speaker';
  import MicVocal from '@lucide/svelte/icons/mic-vocal';
  import ListMusic from '@lucide/svelte/icons/list-music';
  import PictureInPicture2 from '@lucide/svelte/icons/picture-in-picture-2';
  import Ellipsis from '@lucide/svelte/icons/ellipsis';
  import { syncLibrary, toggleMiniPlayer } from '#lib/services/tauri.js';

  interface Props {
    onSearch?: (query: string, activate?: boolean) => void;
  }

  let { onSearch }: Props = $props();

  let selectedDevice = $state<string>('');
  let showDeviceMenu = $state(false);
  // Compact-width overflow menu: below the breakpoint the four
  // lower-priority controls collapse into an Ellipsis dropdown so the
  // always-visible controls keep their full tap-target size.
  let overflowOpen = $state(false);

  /** Fetch the current device list from the backend. */
  async function refreshDevices(): Promise<void> {
    try {
      const devices = await getOutputDevices();
      setAudioDevices(devices);
    } catch {
      // Tauri not available
    }
  }

  onMount(() => {
    refreshDevices();
  });

  function handleClickOutside(event: MouseEvent): void {
    const target = event.target as HTMLElement;
    if (!target.closest('.device-selector')) {
      showDeviceMenu = false;
    }
  }

  $effect(() => {
    if (showDeviceMenu) {
      document.addEventListener('click', handleClickOutside);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  });

  function handleOverflowClickOutside(event: MouseEvent): void {
    const target = event.target as HTMLElement;
    if (!target.closest('.overflow-container')) {
      overflowOpen = false;
    }
  }

  $effect(() => {
    if (overflowOpen) {
      document.addEventListener('click', handleOverflowClickOutside);
      return () => document.removeEventListener('click', handleOverflowClickOutside);
    }
  });

  // Crossing the compact breakpoint swaps which menu triggers are visible
  // (Ellipsis vs. the standalone device button), so close both dropdowns —
  // otherwise one lingers invisibly behind its now-hidden button with its
  // document click listener still attached.
  $effect(() => {
    const mq = window.matchMedia('(max-width: 880px)');
    const handleChange = (): void => {
      overflowOpen = false;
      showDeviceMenu = false;
    };
    mq.addEventListener('change', handleChange);
    return () => mq.removeEventListener('change', handleChange);
  });

  function handleSettings(): void {
    setCurrentSection('settings');
    navigateTo({ section: 'settings', selectedArtistId: null, selectedAlbumId: null });
  }

  // Only a sync started from this button toasts its result; scheduled syncs
  // stay silent and the spinning icon is their feedback.
  // The spinner/disabled state below reads getIsSyncing() from the shared
  // store, not a local flag — so it also lights up for a sync triggered from
  // Settings or the background scheduler, not just this button. The actual
  // library refresh happens once, off the library-sync-completed event
  // (syncStatus store), whoever started the sync.
  async function handleSyncLibrary(): Promise<void> {
    if (getIsSyncing()) return;
    try {
      const m = await syncLibrary();
      if (!m.skipped) {
        toast.success('Library synced', {
          description: `${m.artistCount} artists, ${m.albumCount} albums, ${m.trackCount} tracks`,
        });
      }
    } catch (e) {
      console.error('Library sync failed:', e);
      toast.error('Library sync failed', {
        description: e instanceof Error ? e.message : String(e),
      });
    }
  }

  function handleOfflineToggle(): void {
    setOfflineMode(!getOfflineMode());
  }

  async function handleDeviceChange(deviceName: string): Promise<void> {
    const previousDevice = selectedDevice;
    selectedDevice = deviceName;
    showDeviceMenu = false;
    overflowOpen = false;
    try {
      await setOutputDevice(deviceName || null);
    } catch (e) {
      selectedDevice = previousDevice;
      console.error('Output device change failed:', e);
      toast.error('Could not switch the audio output', {
        description: e instanceof Error ? e.message : String(e),
      });
    }
  }

  async function handleToggleMiniPlayer(): Promise<void> {
    try {
      await toggleMiniPlayer();
    } catch (e) {
      console.error('Mini player toggle failed:', e);
      toast.error('Could not open the mini player', {
        description: e instanceof Error ? e.message : String(e),
      });
    }
  }

</script>

<div class="right-group">
  <div class="search-container">
    <SearchBar {onSearch} />
  </div>
  <button
    class="toolbar-btn mp-control"
    class:syncing={getIsSyncing()}
    onclick={handleSyncLibrary}
    aria-busy={getIsSyncing()}
    aria-label="Sync library"
    title={getIsSyncing() ? 'Syncing library…' : 'Sync library from Jellyfin'}
  >
    <RefreshCw size={18} />
  </button>
  <button
    class="toolbar-btn lp-control"
    class:active={getOfflineMode()}
    onclick={handleOfflineToggle}
    aria-label={getOfflineMode() ? 'Go online' : 'Go offline'}
    title={getOfflineMode() ? 'Offline mode (click to go online)' : 'Online (click to go offline)'}
  >
    {#if getOfflineMode()}
      <WifiOff size={18} />
    {:else}
      <Wifi size={18} />
    {/if}
  </button>
  <button
    class="toolbar-btn mp-control"
    class:active={getLyricsVisible()}
    onclick={toggleLyrics}
    disabled={!getPlaybackState().currentTrack}
    aria-label={getLyricsVisible() ? 'Hide lyrics' : 'Show lyrics'}
    title={getLyricsVisible() ? 'Hide lyrics' : 'Show lyrics'}
  >
    <MicVocal size={18} />
  </button>
  <button
    class="toolbar-btn mp-control"
    class:active={getUpNextVisible()}
    onclick={toggleUpNext}
    aria-label={getUpNextVisible() ? 'Hide Up Next' : 'Show Up Next'}
    title={getUpNextVisible() ? 'Hide Up Next (⌘/Ctrl+U)' : 'Show Up Next (⌘/Ctrl+U)'}
  >
    <ListMusic size={18} />
  </button>
  <button
    class="toolbar-btn lp-control"
    onclick={handleToggleMiniPlayer}
    aria-label="Open MiniPlayer"
    title="MiniPlayer (Ctrl/Cmd+Shift+M)"
  >
    <PictureInPicture2 size={18} />
  </button>
  <div class="device-selector lp-control">
    <button
      class="toolbar-btn"
      onclick={() => { showDeviceMenu = !showDeviceMenu; if (!showDeviceMenu) return; refreshDevices(); }}
      aria-label="Audio output device"
      title="Audio output: {getAudioDevices().find(d => d.name === selectedDevice)?.description ?? 'System Default'}"
    >
      <Speaker size={18} />
    </button>
    {#if showDeviceMenu}
      <div class="device-menu">
        <button
          class="device-option"
          class:selected={!selectedDevice}
          onclick={() => handleDeviceChange('')}
        >
          System Default
        </button>
        {#each getAudioDevices() as device}
          <button
            class="device-option"
            class:selected={selectedDevice === device.name}
            onclick={() => handleDeviceChange(device.name)}
          >
            {device.description}
            {#if device.isDefault}
              <span class="device-default">(Default)</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}
  </div>
  <button
    class="toolbar-btn lp-control"
    class:active={getUiState().currentSection === 'settings'}
    onclick={handleSettings}
    aria-label="Settings"
    title="Settings"
  >
    <Settings size={18} />
  </button>
  <!-- Overflow menu: only rendered ≤880px, where the four lp-control
       buttons above are hidden, and ≤640px the three mp-control ones too.
       Surfaces them (plus the device list, inlined so no nested flyout is
       needed) without shrinking the visible controls. -->
  <div class="overflow-container">
    <button
      class="toolbar-btn"
      class:active={overflowOpen}
      onclick={() => { overflowOpen = !overflowOpen; if (overflowOpen) refreshDevices(); }}
      aria-label="More controls"
      aria-expanded={overflowOpen}
      title="More controls"
    >
      <Ellipsis size={18} />
    </button>
    {#if overflowOpen}
      <div class="overflow-menu">
        <button
          class="overflow-option mp-entry"
          onclick={() => { overflowOpen = false; handleSyncLibrary(); }}
          aria-busy={getIsSyncing()}
        >
          <RefreshCw size={16} />
          <span>{getIsSyncing() ? 'Syncing library…' : 'Sync library'}</span>
        </button>
        <button
          class="overflow-option mp-entry"
          class:selected={getLyricsVisible()}
          onclick={() => { overflowOpen = false; toggleLyrics(); }}
          disabled={!getPlaybackState().currentTrack}
        >
          <MicVocal size={16} />
          <span>{getLyricsVisible() ? 'Hide lyrics' : 'Show lyrics'}</span>
        </button>
        <button
          class="overflow-option mp-entry"
          class:selected={getUpNextVisible()}
          onclick={() => { overflowOpen = false; toggleUpNext(); }}
        >
          <ListMusic size={16} />
          <span>{getUpNextVisible() ? 'Hide Up Next' : 'Show Up Next'}</span>
        </button>
        <button
          class="overflow-option"
          class:selected={getOfflineMode()}
          onclick={handleOfflineToggle}
        >
          {#if getOfflineMode()}
            <WifiOff size={16} />
            <span>Offline mode</span>
          {:else}
            <Wifi size={16} />
            <span>Online</span>
          {/if}
        </button>
        <button
          class="overflow-option"
          onclick={() => { overflowOpen = false; handleToggleMiniPlayer(); }}
        >
          <PictureInPicture2 size={16} />
          <span>MiniPlayer</span>
        </button>
        <button
          class="overflow-option"
          class:selected={getUiState().currentSection === 'settings'}
          onclick={() => { overflowOpen = false; handleSettings(); }}
        >
          <Settings size={16} />
          <span>Settings</span>
        </button>
        <div class="overflow-divider"></div>
        <div class="overflow-section-label">Audio output</div>
        <button
          class="overflow-option"
          class:selected={!selectedDevice}
          onclick={() => handleDeviceChange('')}
        >
          <Speaker size={16} />
          <span>System Default</span>
        </button>
        {#each getAudioDevices() as device}
          <button
            class="overflow-option"
            class:selected={selectedDevice === device.name}
            onclick={() => handleDeviceChange(device.name)}
          >
            <Speaker size={16} />
            <span>{device.description}{#if device.isDefault}&nbsp;(Default){/if}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  /* PlayerBar's --side-reserve is this group's width (312px, and 178px once
     folded): change one, change the other. */
  .right-group {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-sm);
    padding-right: var(--space-lg);
    min-width: 0;
  }

  .search-container {
    display: flex;
    align-items: center;
  }

  .toolbar-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-muted);
    cursor: pointer;
    flex-shrink: 0;
    transition:
      color var(--transition-fast),
      background-color var(--transition-fast);
  }

  .toolbar-btn:hover {
    color: var(--p11-text-primary);
    background-color: var(--p11-bg-hover);
  }

  .toolbar-btn.active {
    color: var(--p11-accent);
    background-color: var(--p11-accent-subtle);
  }

  .toolbar-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .toolbar-btn:disabled:hover {
    color: var(--p11-text-muted);
    background-color: transparent;
  }

  .toolbar-btn.syncing {
    color: var(--p11-accent);
    cursor: wait;
  }

  .toolbar-btn.syncing :global(svg) {
    animation: spin 1s linear infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .toolbar-btn.syncing :global(svg) {
      animation: none;
    }
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  /* Device selector dropdown */
  .device-selector {
    position: relative;
  }

  .device-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: var(--space-xs);
    min-width: 220px;
    max-width: 320px;
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    box-shadow: var(--p11-shadow-md);
    z-index: 100;
    overflow: hidden;
  }

  .device-option {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    background: transparent;
    border: none;
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    font-family: var(--font-family);
    text-align: left;
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .device-option:hover {
    background-color: var(--p11-bg-hover);
    color: var(--p11-text-primary);
  }

  .device-option.selected {
    background-color: var(--p11-accent-subtle);
    color: var(--p11-accent);
  }

  .device-default {
    color: var(--p11-text-muted);
    font-size: var(--font-size-xs);
  }

  /* Overflow menu (compact widths only) — mirrors the device dropdown. */
  .overflow-container {
    position: relative;
    display: none;
  }

  .overflow-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: var(--space-xs);
    min-width: 220px;
    max-width: 320px;
    padding: var(--space-xs) 0;
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    box-shadow: var(--p11-shadow-md);
    z-index: 100;
    overflow: hidden;
  }

  .overflow-option {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    width: 100%;
    min-width: 0;
    padding: var(--space-sm) var(--space-md);
    background: transparent;
    border: none;
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    font-family: var(--font-family);
    text-align: left;
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .overflow-option:hover:not(:disabled) {
    background-color: var(--p11-bg-hover);
    color: var(--p11-text-primary);
  }

  .overflow-option.selected {
    background-color: var(--p11-accent-subtle);
    color: var(--p11-accent);
  }

  .overflow-option:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .overflow-option.mp-entry {
    display: none;
  }

  .overflow-option :global(svg) {
    flex-shrink: 0;
  }

  .overflow-option span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .overflow-divider {
    height: 1px;
    margin: var(--space-xs) 0;
    background-color: var(--p11-border);
  }

  .overflow-section-label {
    padding: var(--space-xs) var(--space-md);
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  /* Compact mode (shared with PlayerBar): fold the four lower-priority
     controls into the overflow menu and reveal its trigger. The remaining
     controls keep their 30px baseline — nothing shrinks below the tap
     target. */
  @container app (width <= 67.7rem) {
    .right-group {
      gap: var(--space-xs);
      padding-right: var(--space-md);
    }

    .lp-control {
      display: none;
    }

    .overflow-container {
      display: flex;
    }
  }

  /* Narrow mode: sync, lyrics and Up Next fold too, leaving search and the
     overflow trigger, so the now-playing panel keeps its 176px down to the
     520px window minimum (PlayerBar). */
  @container app (width <= 49.2rem) {
    .mp-control {
      display: none;
    }

    .overflow-option.mp-entry {
      display: flex;
    }
  }
</style>
