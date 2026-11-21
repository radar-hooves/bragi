<script lang="ts">
  import '../app.css';
  import '#lib/styles/tokens.css';
  import { ModeWatcher } from 'mode-watcher';
  import {
    Sidebar,
    PlayerBar,
    WindowControls,
    LyricsPanel,
    UpNextPanel,
    FullScreenPlayer,
    MiniPlayer,
  } from '#lib/components/layout/index.js';
  import { getLyricsVisible } from '#lib/stores/lyrics.svelte.js';
  import { CreatePlaylistDialog, AiPlaylistDialog, ImportPlaylistDialog } from '#lib/components/playlist/index.js';
  import { Toaster } from '@poodle64/ui/sonner';
  import { setupEventListeners, cleanupEventListeners, initAppState } from '#lib/services/index.js';
  import { initScrobbleState } from '#lib/stores/scrobble.svelte.js';
  import { initJellyfinState } from '#lib/stores/jellyfin.svelte.js';
  import { loadJellyfinPlaylists } from '#lib/stores/jellyfinPlaylist.svelte.js';
  import { setCurrentSection, loadUIState, getUiState, toggleColumnBrowser, stepUiZoomLevel, resetUiZoomLevel, setThemeMode, getUpNextVisible, toggleUpNext } from '#lib/stores/ui.svelte.js';
  import { bindScrollMemory } from '#lib/utils/scrollMemory.svelte.js';
  import { loadPlaybackPreferences, updatePlaybackState, getPlaybackState } from '#lib/stores/player.svelte.js';
  import { loadPreferences } from '#lib/stores/preferences.svelte.js';
  import { setQueueState } from '#lib/stores/queue.svelte.js';
  import { initOfflineMode } from '#lib/stores/offlineMode.svelte.js';
  import { loadRecentlyPlayed } from '#lib/stores/recentlyPlayed.svelte.js';
  import { loadEarwormStats, loadServerPlayCounts } from '#lib/stores/trackMetadata.svelte.js';
  import { loadLyricsOffset } from '#lib/stores/lyrics.svelte.js';
  import { navigateBack, navigateForward } from '#lib/stores/navigation.svelte.js';
  import { togglePlayPause } from '#lib/utils/playback.js';
  import { revealTrackInLibrary } from '#lib/utils/revealInLibrary.svelte.js';
  import { handleGlobalShortcut } from '#lib/utils/globalShortcuts.js';
  import { onMount, onDestroy } from 'svelte';
  import { startUpdateChecks } from '@poodle64/ui/updater';
  import { emit } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    quitApp,
    toggleMiniPlayer,
    getPlaybackState as fetchPlaybackState,
    getQueue,
    getSettings,
  } from '#lib/services/tauri.js';
  import type { Snippet } from 'svelte';

  interface Props {
    children?: Snippet;
  }

  let { children }: Props = $props();

  // Platform detection for Linux-only features
  const isLinux = typeof navigator !== 'undefined' && navigator.userAgent.toLowerCase().includes('linux');

  // The MiniPlayer is a second Tauri window (label "mini") that loads this
  // same SPA. Branch on the window label to render the compact controller and
  // run a trimmed init — it must NOT boot the full app shell or a second sync.
  let windowLabel = 'main';
  try {
    windowLabel = getCurrentWindow().label;
  } catch {
    // Not running under Tauri (browser dev) — treat as the main window.
  }
  const isMiniPlayer = windowLabel === 'mini';

  // Show window controls only on Linux when decorations are disabled
  const showWindowControls = $derived(isLinux && !getUiState().titleBarVisible);

  // Every list-shaped view scrolls in its own internal viewport and remembers
  // its own position; the two that don't (search results, the download
  // queue) flow the whole page and scroll this shared `.content` region
  // instead, so it remembers position per section here.
  let contentEl = $state<HTMLElement>();
  $effect(() => bindScrollMemory(`content:${getUiState().currentSection}`, contentEl));

  let showCreatePlaylistDialog = $state(false);
  let showAiPlaylistDialog = $state(false);
  let showImportPlaylistDialog = $state(false);

  // Keyboard handler reference for cleanup
  let keydownHandler: ((event: KeyboardEvent) => void) | null = null;
  let stopUpdateChecks: (() => void) | null = null;

  onMount(async () => {
    // Set platform attribute for CSS-based platform detection
    document.documentElement.setAttribute('data-platform', isLinux ? 'linux' : 'macos');

    // MiniPlayer window: a lightweight controller. Wire the playback event
    // listeners and hydrate the current state once (it may open mid-playback),
    // but skip the icon, UI-state load, and the whole Phase-2 network boot —
    // the main window already owns those, and a second library sync would be
    // wasteful. Playback stays in sync because the engine broadcasts to all
    // windows. Do NOT emit 'frontend-ready' (audio is already initialised).
    if (isMiniPlayer) {
      // ownsSideEffects: false — the mini gets live UI updates but must NOT
      // scrobble, record history, or fire toasts (the main window owns those).
      await setupEventListeners({ ownsSideEffects: false });
      try {
        const [playback, queue, settings] = await Promise.all([fetchPlaybackState(), getQueue(), getSettings()]);
        updatePlaybackState(playback);
        setQueueState(queue);
        // This window is a separate webview from the main one: apply the
        // persisted theme mode explicitly rather than relying on ModeWatcher's
        // own localStorage default, which would otherwise miss an explicit
        // light/dark override made in the main window's Settings.
        setThemeMode(settings.theme === 'light' || settings.theme === 'dark' ? settings.theme : 'system');
      } catch {
        // Not running under Tauri — nothing to hydrate.
      }
      return;
    }

    // Set window icon (needed for dev mode where no .app bundle exists)
    try {
      await getCurrentWindow().setIcon('icons/128x128@2x.png');
    } catch {
      // Silently ignore — icon may already be set via bundle in production
    }

    // Phase 1 (critical, sequential): UI state that must load before rendering
    await loadUIState();
    await setupEventListeners();
    await emit('frontend-ready');

    // The main window alone checks Bragi's signed releases for a new version.
    stopUpdateChecks = startUpdateChecks();

    // The one global shortcut layer: every main-window
    // shortcut — Quit, MiniPlayer, column browser, reveal-in-library,
    // back/forward, play/pause, focus-search — is wired here, and
    // handleGlobalShortcut() backs off a focused text field for all of them.
    // See globalShortcuts.ts.
    keydownHandler = (event: KeyboardEvent): void => {
      handleGlobalShortcut(event, {
        togglePlayPause: () => {
          togglePlayPause().catch((e) => console.error('Failed to toggle play/pause:', e));
        },
        quit: () => {
          // quitApp(), not window.close(), so it always truly quits
          // regardless of the close-to-tray setting.
          quitApp().catch((e) => console.error('Failed to quit application:', e));
        },
        toggleMiniPlayer: () => {
          toggleMiniPlayer().catch((e) => console.error('Failed to toggle MiniPlayer:', e));
        },
        toggleColumnBrowser,
        toggleUpNext,
        revealPlayingTrack: () => revealTrackInLibrary(getPlaybackState().currentTrack),
        navigateBack,
        navigateForward,
        focusSearch: () => window.dispatchEvent(new CustomEvent('bragi-focus-search')),
        zoomIn: () => stepUiZoomLevel(1),
        zoomOut: () => stepUiZoomLevel(-1),
        zoomReset: resetUiZoomLevel,
      });
    };
    window.addEventListener('keydown', keydownHandler);

    // Phase 2 (background, parallel): network-dependent state that must not block UI
    Promise.allSettled([
      initJellyfinState(),
      initScrobbleState(),
      loadJellyfinPlaylists(),
      initOfflineMode(),
      loadRecentlyPlayed(),
      loadPlaybackPreferences(),
      loadPreferences(),
      loadLyricsOffset(),
      initAppState(),
      loadEarwormStats(),
      loadServerPlayCounts(),
    ]).catch(() => {
      // allSettled never rejects, but guard defensively
    });
  });

  onDestroy(() => {
    if (keydownHandler) {
      window.removeEventListener('keydown', keydownHandler);
    }
    stopUpdateChecks?.();
    cleanupEventListeners();
  });

  function handleSearch(query: string, activate = false): void {
    window.dispatchEvent(new CustomEvent('bragi-search', { detail: { query, activate } }));
  }

  function handleCreatePlaylist(): void {
    showCreatePlaylistDialog = true;
  }

  function handleCloseCreatePlaylist(): void {
    showCreatePlaylistDialog = false;
  }

  function handleCreateAiPlaylist(): void {
    showAiPlaylistDialog = true;
  }

  function handleCloseAiPlaylist(): void {
    showAiPlaylistDialog = false;
  }

  function handleImportPlaylist(): void {
    showImportPlaylistDialog = true;
  }

  function handleCloseImportPlaylist(): void {
    showImportPlaylistDialog = false;
  }
</script>

<ModeWatcher />

{#if isMiniPlayer}
  <!-- MiniPlayer window: compact controller only — no sidebar/library.
       Its own Toaster surfaces playback errors while the main window is hidden. -->
  <MiniPlayer />
  <Toaster />
{:else}
  <div class="app">
    <PlayerBar onSearch={handleSearch} />
    {#if showWindowControls}
      <div class="window-controls-container">
        <WindowControls />
      </div>
    {/if}
    <div class="main-area">
      <Sidebar
        onCreatePlaylist={handleCreatePlaylist}
        onCreateAiPlaylist={handleCreateAiPlaylist}
        onImportPlaylist={handleImportPlaylist}
      />
      <main class="content" bind:this={contentEl}>
        {#if children}
          {@render children()}
        {/if}
      </main>
      {#if getLyricsVisible()}
        <LyricsPanel />
      {/if}
      {#if getUpNextVisible()}
        <UpNextPanel />
      {/if}
    </div>
  </div>

  <Toaster />

  <!-- Always mounted so its blurred backdrop layer is pre-warmed on the GPU; it
       self-hides (opacity/pointer-events) until opened, so opening is instant. -->
  <FullScreenPlayer />

  {#if showCreatePlaylistDialog}
    <CreatePlaylistDialog onClose={handleCloseCreatePlaylist} />
  {/if}

  {#if showAiPlaylistDialog}
    <AiPlaylistDialog onClose={handleCloseAiPlaylist} />
  {/if}

  {#if showImportPlaylistDialog}
    <ImportPlaylistDialog onClose={handleCloseImportPlaylist} />
  {/if}
{/if}

<style>
  /* The top bar folds by this container's width, in rem: WebKit compares a px
     condition against the unzoomed window, so UI zoom (CSS zoom on the root) would
     narrow the layout without the bar folding, while rem scales with zoom. The
     rem values are the old 880/688/640px breakpoints at the 13px root size. */
  .app {
    container: app / inline-size;
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    background-color: var(--p11-bg-primary);
    position: relative;
  }

  .window-controls-container {
    position: absolute;
    top: var(--space-sm);
    left: var(--space-sm);
    z-index: 1000;
  }

  .main-area {
    flex: 1;
    display: flex;
    min-height: 0;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  .main-area > :global(aside.sidebar) {
    flex-shrink: 0;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
    min-width: 0;
  }

</style>
