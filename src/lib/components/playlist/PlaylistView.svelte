<script lang="ts">
  import { untrack } from 'svelte';
  import {
    getJellyfinPlaylistState,
    deleteJellyfinPlaylist,
    renameJellyfinPlaylist,
    removeTracksFromJellyfinPlaylist,
    moveJellyfinPlaylistItem,
    clearCurrentJellyfinPlaylist,
    getPendingPlaylistIntent,
    clearPendingPlaylistIntent,
  } from '#lib/stores/jellyfinPlaylist.svelte.js';
  import { getPlaybackState } from '#lib/stores/player.svelte.js';
  import type { JellyfinPlaylistTrack, Track } from '#lib/types/models.js';
  import { Spinner, StatusBar, type StatusBarData } from '#lib/components/common/index.js';
  import GripVertical from '@lucide/svelte/icons/grip-vertical';
  import Play from '@lucide/svelte/icons/play';
  import Pause from '@lucide/svelte/icons/pause';
  import Shuffle from '@lucide/svelte/icons/shuffle';
  import Share2 from '@lucide/svelte/icons/share-2';
  import PlaylistCollage from './PlaylistCollage.svelte';
  import SharePlaylistDialog from './SharePlaylistDialog.svelte';
  import TrackContextMenu from '../library/TrackContextMenu.svelte';
  import TrackListHeaderMenu from '../library/TrackListHeaderMenu.svelte';
  import {
    type ColumnDef,
    resolveActiveColumns,
    colGridTrack,
    computeFitWidth,
    PINNED_FIRST_COLUMN,
    PLAYLIST_TOGGLEABLE_COLUMNS,
  } from '../library/trackColumns';
  import {
    getPlaylistColumns,
    getPlaylistColumnWidths,
    setPlaylistColumns,
    setPlaylistColumnWidths,
    deletePlaylistColumns,
  } from '#lib/stores/ui.svelte.js';
  import { jellyfinPlaylistTrackToTrack } from '#lib/utils/jellyfinPlaylist.js';
  import { queueTrackNext, queueTrackAtEnd } from '#lib/utils/queueActions.js';
  import { createVirtualList } from '#lib/utils/virtualList.svelte.js';
  import { createSelection } from '#lib/utils/selection.svelte.js';
  import { createListKeyboard } from '#lib/utils/listKeyboard.js';
  import { createColumnHeader } from '#lib/utils/columnHeader.svelte.js';
  import { createReorderDrag } from '#lib/utils/reorderDrag.svelte.js';
  import * as Dialog from '@poodle64/ui/dialog';
  import { toast } from 'svelte-sonner';

  interface Props {
    onTrackPlay?: (track: JellyfinPlaylistTrack) => void;
    onShuffleAll?: () => void;
    onAddToPlaylist?: (tracks: Track[]) => void;
  }

  let { onTrackPlay, onShuffleAll, onAddToPlaylist }: Props = $props();

  // Row menu open state, keyed by playlistItemId — same shared-boolean pattern
  // as ArtistAlbumSection (one TrackContextMenu definition, many rows).
  let menuOpenFor = $state<string | null>(null);

  let deleteOpen = $state(false);
  let isDeleting = $state(false);
  let showShareDialog = $state(false);
  // Rename in place (Apple Music reference: click the playlist name at the top
  // of the window, then enter the new name) — no separate dialog.
  let editingName = $state(false);
  let nameDraft = $state('');
  let isRenaming = $state(false);
  // Songs waiting on the "Remove from playlist?" confirmation. Removing edits
  // the playlist in Jellyfin itself, so every path (the row's remove button,
  // Delete/Backspace on a selection) asks first, as Apple Music does.
  let pendingRemoval = $state<string[]>([]);
  let removalOpen = $state(false);
  let isRemoving = $state(false);

  // ── Drag-to-reorder (the pointer-tracking arithmetic is shared with
  // UpNextPanel via `reorderDrag.svelte.ts`) ──────────
  async function performReorder(from: number, to: number): Promise<void> {
    const state = getJellyfinPlaylistState();
    const pl = state.currentPlaylist;
    if (!pl) return;
    if (state.isOperating) {
      toast("Still saving the last change — try that move again");
      return;
    }

    const moved = pl.tracks[from];
    if (!moved) return;
    const snapshot = pl.tracks.slice();

    // Optimistic reorder: mutate the deeply-reactive array in place (P11 rule —
    // no spread-replace). The keyed {#each} reuses DOM rows across the move.
    pl.tracks.splice(from, 1);
    pl.tracks.splice(to, 0, moved);

    // Jellyfin's Move endpoint takes the playlist ENTRY id (playlistItemId), not
    // the track's Jellyfin id, and `to` as the 0-based final position.
    const ok = await moveJellyfinPlaylistItem(pl.jellyfinId, moved.playlistItemId, to);
    if (!ok) {
      // Revert in place — the store leaves currentPlaylist untouched on failure.
      pl.tracks.splice(0, pl.tracks.length, ...snapshot);
      toast.error("Couldn't reorder this playlist");
    }
    // On success the store replaces currentPlaylist with the authoritative
    // server order, so the optimistic view reconciles automatically.
  }

  const reorder = createReorderDrag({
    onReorder: (from, to) => void performReorder(from, to),
  });

  // Native HTML5 DnD also carries the one shared drag payload (TrackList's
  // own format) so a row can be dropped onto ANOTHER sidebar playlist, not
  // just reordered within this one — layered on top of the shared mechanism,
  // since that payload is PlaylistView's own concern.
  function handleRowDragStart(event: DragEvent, index: number): void {
    reorder.onDragStart(event, index);
    if (!event.dataTransfer) return;
    const track = playlist?.tracks[index];
    if (!track) return;
    event.dataTransfer.setData(
      'application/x-bragi-playlist',
      JSON.stringify({ type: 'tracks', trackIds: tracksForAction(track).map((t) => t.jellyfinId) })
    );
  }

  // Derive whether a playlist is selected from the store
  const hasSelectedPlaylist = $derived(getJellyfinPlaylistState().selectedPlaylistId !== null);

  function handlePlayAll(): void {
    const playlist = getJellyfinPlaylistState().currentPlaylist;
    if (!playlist?.tracks.length) return;

    const firstTrack = playlist.tracks[0];
    if (firstTrack) onTrackPlay?.(firstTrack);
  }

  function handleShuffleAll(): void {
    const playlist = getJellyfinPlaylistState().currentPlaylist;
    if (!playlist?.tracks.length) return;
    onShuffleAll?.();
  }

  function handleTrackPlay(track: JellyfinPlaylistTrack): void {
    onTrackPlay?.(track);
  }

  // Now-playing indicator: matches TrackRow's eq-bar animation so the
  // playlist view is consistent with every other track list.
  function isTrackCurrent(t: JellyfinPlaylistTrack): boolean {
    return getPlaybackState().currentTrack?.jellyfinId === t.jellyfinId;
  }

  function isTrackPlaying(t: JellyfinPlaylistTrack): boolean {
    return isTrackCurrent(t) && getPlaybackState().isPlaying;
  }

  // A menu or drag on a row that's part of a multi-selection acts on the
  // WHOLE selection, matching TrackList's own tracksForAction — a row outside the current selection still acts on just itself
  // (`selection.targetIds`).
  function tracksForAction(track: JellyfinPlaylistTrack): JellyfinPlaylistTrack[] {
    return selection.targetIds(track.playlistItemId).map((id) => trackByItemId.get(id)).filter((t): t is JellyfinPlaylistTrack => !!t);
  }

  /** tracksForAction, converted for the mechanisms (queueActions, the Add to
   * Playlist dialog) that take a plain Track, not a JellyfinPlaylistTrack. */
  function tracksForActionAsTracks(track: JellyfinPlaylistTrack): Track[] {
    return tracksForAction(track).map(jellyfinPlaylistTrackToTrack).filter((t): t is Track => t !== null);
  }

  function requestRemoval(ids: string[]): void {
    if (ids.length === 0) return;
    pendingRemoval = ids;
    removalOpen = true;
  }

  async function confirmRemoval(): Promise<void> {
    if (!playlist) return;
    isRemoving = true;
    try {
      await removeTracksFromJellyfinPlaylist(playlist.jellyfinId, pendingRemoval);
      selection.clear();
    } catch (e) {
      console.error('Failed to remove tracks from playlist:', e);
      toast.error("Couldn't remove the songs from this playlist");
    } finally {
      isRemoving = false;
      removalOpen = false;
      pendingRemoval = [];
    }
  }

  function handleDeleteClick(): void {
    deleteOpen = true;
  }

  async function confirmDelete(): Promise<void> {
    const playlist = getJellyfinPlaylistState().currentPlaylist;
    if (!playlist) return;

    isDeleting = true;
    const success = await deleteJellyfinPlaylist(playlist.jellyfinId);
    isDeleting = false;
    deleteOpen = false;

    if (success) {
      deletePlaylistColumns(playlist.jellyfinId);
      clearCurrentJellyfinPlaylist();
    }
  }

  function startRename(): void {
    if (!playlist) return;
    nameDraft = playlistName;
    editingName = true;
  }

  function cancelRename(): void {
    editingName = false;
    nameDraft = '';
  }

  async function commitRename(): Promise<void> {
    // Disabling the focused field blurs it, and blur commits too.
    if (isRenaming) return;
    const trimmed = nameDraft.trim();
    if (!playlist || !trimmed || trimmed === playlistName) {
      cancelRename();
      return;
    }
    isRenaming = true;
    const success = await renameJellyfinPlaylist(playlist.jellyfinId, trimmed);
    isRenaming = false;
    if (!success) {
      toast.error(getJellyfinPlaylistState().error ?? "Couldn't rename this playlist");
    }
    editingName = false;
    nameDraft = '';
  }

  function handleNameKeyDown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      void commitRename();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      cancelRename();
    }
  }

  const playlist = $derived(getJellyfinPlaylistState().currentPlaylist);
  const trackCount = $derived(playlist?.tracks.length ?? 0);

  // A large playlist can reach thousands of rows (see statusData below), so it
  // routes through the shared virtualiser like every other long list.
  // Row height isn't a known constant here (unlike TrackList's fixed 40px), so
  // it's measured from the real rendered stride, seeded with TrackRow's height.
  let viewportEl = $state<HTMLDivElement>();
  let innerEl = $state<HTMLDivElement>();

  const virtual = createVirtualList({
    count: () => trackCount,
    estimateRowHeight: () => 40,
    overscan: 8,
  });

  interface RenderRow {
    track: JellyfinPlaylistTrack;
    index: number;
    /** Off-window but kept mounted for an in-flight drag — see below. */
    keepAlive: boolean;
  }

  // Both WebKitGTK and WKWebView implement the HTML DnD spec's recommended
  // autoscroll: holding a drag near the scroll container's top/bottom edge
  // scrolls it natively, outside any handler this app writes. That can carry
  // the dragged row's index out of [virtual.start, virtual.end) mid-drag; the
  // keyed {#each} would then destroy its DOM node, and a browser drag session
  // dies the instant its source node leaves the document. So while a drag is
  // in flight, the dragged track's row is always kept in the render list —
  // absolutely positioned out of flow by .keep-alive so it can't displace the
  // rows actually in the window — even once autoscroll has carried it out of
  // range.
  let visibleRows = $derived.by((): RenderRow[] => {
    if (!playlist) return [];
    const tracks = playlist.tracks;
    const rows: RenderRow[] = [];
    for (let i = virtual.start; i < virtual.end; i++) {
      const track = tracks[i];
      if (track) rows.push({ track, index: i, keepAlive: false });
    }
    const draggingIndex = reorder.draggingIndex;
    if (draggingIndex !== null && (draggingIndex < virtual.start || draggingIndex >= virtual.end)) {
      const track = tracks[draggingIndex];
      if (track) rows.push({ track, index: draggingIndex, keepAlive: true });
    }
    return rows;
  });

  $effect(() => virtual.attachViewport(viewportEl));
  $effect(() => virtual.bindScrollMemory(playlist ? `playlist:${playlist.jellyfinId}` : 'playlist', viewportEl));

  $effect(() => {
    trackCount; // read the dependency — a track added/removed shifts the range
    virtual.refresh();
  });

  $effect(() => {
    void virtual.windowing;
    void virtual.end;
    virtual.remeasure(innerEl);
  });

  // One selection model and one list-keyboard handler, keyed
  // by playlistItemId — not jellyfinId, since the same song can appear twice
  // in one playlist under two different entries.
  const trackIds = $derived.by(() => {
    const list = playlist?.tracks ?? [];
    return untrack(() => list.map((t) => t.playlistItemId));
  });
  const trackByItemId = $derived.by(() => {
    const list = playlist?.tracks ?? [];
    return untrack(() => new Map(list.map((t) => [t.playlistItemId, t])));
  });

  const selection = createSelection(() => trackIds);

  function revealActiveRow(index: number): void {
    if (!viewportEl) return;
    const id = trackIds[index];
    // Still mounted: scrollIntoView for the exact "nearest edge" arrow-key
    // behaviour. Scrolled out of the rendered window, jump the virtualiser
    // there directly — the same fallback ColumnBrowserColumn's own list uses.
    if (id && index >= virtual.start && index < virtual.end) {
      viewportEl.querySelector(`[data-playlist-item-id="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'nearest' });
    } else {
      virtual.scrollToIndex(index, 'center');
    }
  }

  const listKeyboard = createListKeyboard({
    getIds: () => trackIds,
    selection,
    getLabel: (id) => trackByItemId.get(id)?.name ?? '',
    // Return plays the selection, starting from the first selected track —
    // the same mechanism the title button and Play All use.
    onActivate: (ids) => {
      const track = trackByItemId.get(ids[0]!);
      if (track) onTrackPlay?.(track);
    },
    // Delete removes the selection from THIS playlist — never the library —
    // after the same confirmation the row's remove button asks for.
    onDelete: (ids) => requestRemoval(ids),
    revealIndex: revealActiveRow,
  });

  function handleRowClick(track: JellyfinPlaylistTrack, event: MouseEvent): void {
    selection.click(track.playlistItemId, { shiftKey: event.shiftKey, metaKey: event.metaKey, ctrlKey: event.ctrlKey });
  }

  // ── Columns ────────────────────────────────────────
  // The same column model TrackList's other lists use — show/hide, reorder,
  // resize, remembered — scoped per playlist, confirmed live against Apple
  // Music: "the options you select will apply to that playlist only, and
  // have to be re-selected for each playlist." Only what a playlist ENTRY can
  // actually supply is offered (`PLAYLIST_TOGGLEABLE_COLUMNS`): Jellyfin's
  // playlist payload carries no track/disc number, genre or year — Last
  // Played/Plays/Skips ARE available, from the local per-track metadata
  // store keyed by jellyfinId, which every entry carries.
  const visibleColumns = $derived(getPlaylistColumns(playlist?.jellyfinId ?? ''));
  const columnWidths = $derived(getPlaylistColumnWidths(playlist?.jellyfinId ?? ''));
  const activeColumns = $derived(resolveActiveColumns(visibleColumns));

  function handleColumnsChange(columns: string[]): void {
    if (!playlist) return;
    setPlaylistColumns(playlist.jellyfinId, columns);
  }

  function handleColumnWidthsChange(widths: Record<string, number>): void {
    if (!playlist) return;
    setPlaylistColumnWidths(playlist.jellyfinId, widths);
  }

  // Fit-to-content measurement (the shared column-header mechanism, reused here): a canvas
  // context using this list's own row font.
  let measureCanvas: HTMLCanvasElement | null = null;
  function getMeasureContext(): CanvasRenderingContext2D | null {
    if (!measureCanvas) measureCanvas = document.createElement('canvas');
    const ctx = measureCanvas.getContext('2d');
    if (ctx) {
      const sample = viewportEl?.querySelector<HTMLElement>('.track-row');
      if (sample) ctx.font = getComputedStyle(sample).font;
    }
    return ctx;
  }

  function measureFitWidth(col: ColumnDef): number {
    const ctx = getMeasureContext();
    if (!ctx) return col.minWidth;
    const values = (playlist?.tracks ?? []).map((t) => {
      const asTrack = jellyfinPlaylistTrackToTrack(t);
      return asTrack ? col.render(asTrack) : '';
    });
    return computeFitWidth(values, (text) => ctx.measureText(text).width, col.minWidth);
  }

  const columnHeader = createColumnHeader({
    getColumns: () => activeColumns,
    getWidths: () => columnWidths,
    onWidthsChange: handleColumnWidthsChange,
    onOrderChange: handleColumnsChange,
    pinnedFirstColumn: PINNED_FIRST_COLUMN,
    measureFitWidth,
  });

  // Merges the live-drag widths over the committed ones so a resize in
  // progress moves this grid track immediately, not only once it commits on
  // mouseup — the same live width TrackList's own <col> reads via `widthFor`.
  const gridTemplate = $derived(
    [
      '22px',
      '32px',
      ...activeColumns.map((c) => colGridTrack(c, { ...columnWidths, ...columnHeader.liveWidths })),
      '40px',
    ].join(' ')
  );

  let columnMenuPosition = $state<{ x: number; y: number } | null>(null);

  function handleHeaderContextMenu(event: MouseEvent): void {
    event.preventDefault();
    columnMenuPosition = { x: event.clientX, y: event.clientY };
  }

  function handleToggleColumn(columnId: string): void {
    const current = [...visibleColumns];
    const idx = current.indexOf(columnId);
    if (idx >= 0) current.splice(idx, 1);
    else current.push(columnId);
    handleColumnsChange(current);
  }

  // The playlist name lives on the summary list, not the detail payload (the
  // backend skips a redundant metadata request), so resolve it by ID here.
  const playlistName = $derived.by(() => {
    const state = getJellyfinPlaylistState();
    const id = playlist?.jellyfinId ?? state.selectedPlaylistId;
    return state.playlists.find((p) => p.jellyfinId === id)?.name ?? '';
  });

  const statusData = $derived.by((): StatusBarData | null => {
    if (!playlist || trackCount === 0) return null;
    const list = playlist.tracks;
    const count = trackCount;
    // Sum over the deeply-reactive playlist tracks under untrack() (see the
    // status-footer O(n^2) fix); large playlists can reach thousands of rows.
    return untrack(() => {
      let totalMs = 0;
      for (const track of list) {
        totalMs += track.durationMs;
      }
      return {
        itemCount: count,
        itemLabel: 'song',
        durationMs: totalMs,
        sizeBytes: 0, // File size not available for Jellyfin playlist tracks
      };
    });
  });

  // A rename/delete requested from the sidebar row menu (PlaylistList) before
  // this playlist was open — picked up once it loads, the one rename/delete
  // mechanism reused rather than a second copy in the sidebar.
  $effect(() => {
    const intent = getPendingPlaylistIntent();
    if (!intent || !playlist || intent.playlistId !== playlist.jellyfinId) return;
    clearPendingPlaylistIntent();
    if (intent.type === 'rename') startRename();
    else handleDeleteClick();
  });
</script>

<div class="playlist-view">
  {#if !hasSelectedPlaylist && !playlist}
    <div class="empty-state">
      <div class="empty-icon">&#9835;</div>
      <h2>Select a Playlist</h2>
      <p>Choose a playlist from the sidebar to view its tracks.</p>
    </div>
  {:else if getJellyfinPlaylistState().isLoading}
    <div class="loading-state">
      <Spinner size={48} />
      <p>Loading playlist...</p>
    </div>
    <!-- A playlist already on screen stays on screen: a failed mutation
         (rename, remove, reorder, add) is reported by its own toast, never by
         blanking the list the user is looking at behind this generic error
         screen — that's reserved for a playlist that never loaded at all. -->
  {:else if playlist}
    <header class="playlist-header">
      <div class="playlist-info">
        <div class="playlist-artwork">
          <!-- Jellyfin-style collage of the first four distinct album covers;
               the component keeps the ♫ fallback for a genuinely-empty playlist. -->
          <PlaylistCollage tracks={playlist.tracks} />
        </div>
        <div class="playlist-meta">
          {#if editingName}
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="playlist-name-input"
              bind:value={nameDraft}
              maxlength="255"
              autofocus
              aria-label="Playlist name"
              disabled={isRenaming}
              onkeydown={handleNameKeyDown}
              onblur={() => void commitRename()}
            />
          {:else}
            <button class="playlist-name-btn" onclick={startRename} title="Click to rename">
              <h1 class="playlist-name">{playlistName}</h1>
            </button>
          {/if}
          <div class="playlist-stats">
            <span>{trackCount} {trackCount === 1 ? 'track' : 'tracks'}</span>
          </div>
        </div>
      </div>

      <div class="playlist-actions">
        <button
          class="action-btn primary"
          onclick={handlePlayAll}
          disabled={trackCount === 0}
          title="Play all tracks"
        >
          <Play size={18} fill="currentColor" />
          <span>Play All</span>
        </button>
        <button
          class="action-btn"
          onclick={handleShuffleAll}
          disabled={trackCount === 0}
          title="Shuffle all tracks"
        >
          <Shuffle size={18} />
          <span>Shuffle</span>
        </button>
        <button
          class="action-btn"
          onclick={() => (showShareDialog = true)}
          disabled={trackCount === 0}
          title="Share this playlist"
        >
          <Share2 size={18} />
          <span>Share</span>
        </button>
        <button class="action-btn danger" onclick={handleDeleteClick}> Delete </button>
      </div>
    </header>

    {#if trackCount === 0}
      <div class="empty-tracks">
        <p>This playlist is empty.</p>
      </div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="track-header"
        style:grid-template-columns={gridTemplate}
        oncontextmenu={handleHeaderContextMenu}
        onmousedown={columnHeader.handleMouseDown}
        onmousemove={columnHeader.handleMouseMove}
        ondblclick={columnHeader.handleDoubleClick}
      >
        <span class="col-grip"></span>
        <span class="col-number">#</span>
        {#each activeColumns as col (col.id)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span
            class="col-header-cell"
            class:align-right={col.align === 'right'}
            class:drag-over={columnHeader.dragOverColId === col.id}
            data-col={col.id}
            draggable={columnHeader.isDraggable(col.id)}
            ondragstart={(e) => columnHeader.handleDragStart(e, col.id)}
            ondragover={(e) => columnHeader.handleDragOver(e, col.id)}
            ondrop={(e) => columnHeader.handleDrop(e, col.id)}
            ondragend={columnHeader.handleDragEnd}
          >
            {col.label.toUpperCase()}
          </span>
        {/each}
        <span class="col-actions"></span>
      </div>
      <div
        class="track-list"
        role="listbox"
        aria-multiselectable="true"
        bind:this={viewportEl}
        onscroll={virtual.handleScroll}
        tabindex="0"
        onkeydown={listKeyboard.handleKeyDown}
        aria-activedescendant={selection.activeId ? `playlist-row-${selection.activeId}` : undefined}
      >
        <div class="track-list-spacer" style:height={virtual.windowing ? `${virtual.totalSize}px` : undefined}>
          <div
            class="track-list-window"
            class:is-windowed={virtual.windowing}
            bind:this={innerEl}
            style:transform={virtual.windowing ? `translateY(${virtual.padStart}px)` : undefined}
          >
            {#each visibleRows as row (row.track.playlistItemId)}
              {@const { track, index, keepAlive } = row}
              {@const asTrack = jellyfinPlaylistTrackToTrack(track)}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                id="playlist-row-{track.playlistItemId}"
                data-playlist-item-id={track.playlistItemId}
                class="track-row"
                style:grid-template-columns={gridTemplate}
                class:keep-alive={keepAlive}
                class:selected={selection.isSelected(track.playlistItemId)}
                class:active={selection.activeId === track.playlistItemId}
                class:now-playing={isTrackCurrent(track)}
                class:dragging={reorder.draggingIndex === index}
                class:drop-before={reorder.draggingIndex !== null && reorder.dropIndex === index}
                class:drop-after={reorder.draggingIndex !== null &&
                  reorder.dropIndex === playlist.tracks.length &&
                  index === playlist.tracks.length - 1}
                role="option"
                aria-selected={selection.isSelected(track.playlistItemId)}
                aria-hidden={keepAlive}
                draggable={!getJellyfinPlaylistState().isOperating}
                onclick={(e) => handleRowClick(track, e)}
                oncontextmenu={(e) => {
                  e.preventDefault();
                  if (!selection.isSelected(track.playlistItemId)) handleRowClick(track, e);
                  menuOpenFor = track.playlistItemId;
                }}
                ondragstart={(e) => handleRowDragStart(e, index)}
                ondragover={(e) => reorder.onDragOver(e, index)}
                ondrop={reorder.onDrop}
                ondragend={reorder.onDragEnd}
              >
                <span class="col-grip" aria-hidden="true" title="Drag to reorder">
                  <GripVertical size={14} />
                </span>
                <span class="col-number">{index + 1}</span>
                {#each activeColumns as col (col.id)}
                  {#if col.id === 'title'}
                    <button class="col-title track-title-btn" onclick={() => handleTrackPlay(track)}>
                      {#if isTrackPlaying(track)}
                        <span class="playing-indicator" title="Now playing">
                          <span class="eq-bar"></span>
                          <span class="eq-bar"></span>
                          <span class="eq-bar"></span>
                        </span>
                      {:else if isTrackCurrent(track)}
                        <span class="paused-indicator">
                          <Pause size={11} fill="currentColor" />
                        </span>
                      {/if}
                      <span class="track-title-text">{track.name}</span>
                    </button>
                  {:else}
                    <span class="col-cell" class:align-right={col.align === 'right'}>{asTrack ? col.render(asTrack) : ''}</span>
                  {/if}
                {/each}
                <span class="col-actions">
                  {#if !getJellyfinPlaylistState().isOperating && asTrack}
                    <TrackContextMenu
                      track={asTrack}
                      showAddToPlaylist={!!onAddToPlaylist}
                      bind:open={
                        () => menuOpenFor === track.playlistItemId,
                        (v) => { menuOpenFor = v ? track.playlistItemId : null; }
                      }
                      onPlay={() => handleTrackPlay(track)}
                      onQueueNext={() => queueTrackNext(tracksForActionAsTracks(track))}
                      onQueueEnd={() => queueTrackAtEnd(tracksForActionAsTracks(track))}
                      onAddToPlaylist={() => onAddToPlaylist?.(tracksForActionAsTracks(track))}
                      onRemoveFromPlaylist={() => requestRemoval(tracksForAction(track).map((t) => t.playlistItemId))}
                    />
                  {/if}
                </span>
              </div>
            {/each}
          </div>
        </div>
      </div>
    {/if}

    <StatusBar data={statusData} />
  {:else if getJellyfinPlaylistState().error}
    <div class="error-state">
      <p class="error-message">{getJellyfinPlaylistState().error}</p>
    </div>
  {/if}
</div>

{#if showShareDialog && playlist}
  <SharePlaylistDialog
    playlistName={playlistName}
    tracks={playlist.tracks}
    onClose={() => (showShareDialog = false)}
  />
{/if}

{#if columnMenuPosition}
  <TrackListHeaderMenu
    {visibleColumns}
    toggleableColumns={PLAYLIST_TOGGLEABLE_COLUMNS}
    onToggleColumn={handleToggleColumn}
    position={columnMenuPosition}
    onClose={() => (columnMenuPosition = null)}
  />
{/if}

<Dialog.Root bind:open={removalOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Remove from playlist?</Dialog.Title>
      <Dialog.Description>
        {pendingRemoval.length === 1 ? 'This song' : `These ${pendingRemoval.length} songs`} will be removed from
        “{playlistName}”. They stay in your library.
      </Dialog.Description>
    </Dialog.Header>
    <div class="delete-confirm">
      <button class="cancel-btn" onclick={() => (removalOpen = false)} disabled={isRemoving}>Cancel</button>
      <button class="confirm-delete-btn" onclick={() => void confirmRemoval()} disabled={isRemoving}>
        {#if isRemoving}
          <Spinner size={14} />
        {:else}
          Remove
        {/if}
      </button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={deleteOpen}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Delete playlist?</Dialog.Title>
      <Dialog.Description>
        “{playlistName}” will be deleted from Jellyfin and any other app connected to your server.
        This can't be undone.
      </Dialog.Description>
    </Dialog.Header>
    <div class="delete-confirm">
      <button class="cancel-btn" onclick={() => (deleteOpen = false)} disabled={isDeleting}>Cancel</button>
      <button class="confirm-delete-btn" onclick={() => void confirmDelete()} disabled={isDeleting}>
        {#if isDeleting}
          <Spinner size={14} />
        {:else}
          Delete
        {/if}
      </button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<style>
  .playlist-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 0 var(--space-lg);
  }

  .empty-state,
  .loading-state,
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--space-3xl);
    text-align: center;
    flex: 1;
  }

  .empty-icon {
    font-size: var(--font-size-3xl);
    color: var(--p11-text-muted);
    margin-bottom: var(--space-lg);
  }

  .empty-state h2 {
    font-size: var(--font-size-xl);
    font-weight: var(--font-weight-semibold);
    margin: 0 0 var(--space-sm) 0;
    color: var(--p11-text-primary);
  }

  .empty-state p,
  .loading-state p {
    color: var(--p11-text-secondary);
    margin: 0;
  }

  .error-message {
    color: var(--p11-error);
    margin: 0 0 var(--space-lg) 0;
  }

  .playlist-header {
    display: flex;
    gap: var(--space-xl);
    padding-bottom: var(--space-xl);
    border-bottom: 1px solid var(--p11-border);
    margin-bottom: var(--space-lg);
    flex-wrap: wrap;
  }

  .playlist-info {
    display: flex;
    gap: var(--space-lg);
    flex: 1;
    min-width: 300px;
  }

  .playlist-artwork {
    width: 160px;
    height: 160px;
    background-color: var(--p11-bg-elevated);
    border-radius: var(--p11-radius-lg);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    /* Clip the collage tiles to the box's rounded corners. */
    overflow: hidden;
  }

  .playlist-meta {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: var(--space-sm);
  }

  .playlist-name {
    font-size: var(--font-size-2xl);
    font-weight: var(--font-weight-bold);
    margin: 0;
    color: var(--p11-text-primary);
  }

  .playlist-name-btn {
    display: block;
    padding: 0;
    background: transparent;
    border: none;
    text-align: left;
    cursor: text;
    border-radius: var(--p11-radius-sm);
  }

  .playlist-name-btn:hover .playlist-name {
    color: var(--p11-accent);
  }

  .playlist-name-input {
    width: 100%;
    padding: 2px 4px;
    margin: -2px -4px;
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-accent);
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-primary);
    font-size: var(--font-size-2xl);
    font-weight: var(--font-weight-bold);
    font-family: var(--font-family);
  }

  .playlist-name-input:focus {
    outline: none;
  }

  .playlist-stats {
    display: flex;
    gap: var(--space-sm);
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .playlist-actions {
    display: flex;
    gap: var(--space-sm);
    align-items: flex-start;
    flex-wrap: wrap;
  }

  /* Shared button language with the artist header: same pill shape,
     primary/secondary/danger variants, icon+label layout and active-press
     feedback. Backgrounds use theme tokens (not the hero's translucent white)
     since the playlist header sits on the normal app background. */
  .action-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px var(--space-lg);
    background: var(--p11-bg-elevated);
    border: none;
    border-radius: var(--radius-full);
    color: var(--p11-text-primary);
    font-size: var(--font-size-sm);
    font-family: var(--font-family);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition:
      background-color var(--transition-fast),
      transform var(--transition-spring);
  }

  .action-btn:hover:not(:disabled) {
    background: var(--p11-bg-surface);
  }

  .action-btn:active:not(:disabled) {
    transform: scale(0.97);
  }

  .action-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .action-btn.primary {
    background: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
  }

  .action-btn.primary:hover:not(:disabled) {
    background: var(--p11-accent-hover);
  }

  .action-btn.danger {
    color: var(--p11-error);
  }

  .action-btn.danger:hover:not(:disabled) {
    background: var(--p11-error-bg);
  }

  .delete-confirm {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-sm);
    background-color: var(--p11-error-bg);
    border-radius: var(--p11-radius-md);
    font-size: var(--font-size-sm);
    color: var(--p11-error);
  }

  .confirm-delete-btn {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-xs) var(--space-sm);
    background-color: var(--p11-error);
    border: none;
    border-radius: var(--p11-radius-sm);
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-sm);
    cursor: pointer;
  }

  .confirm-delete-btn:disabled {
    opacity: 0.5;
  }

  .cancel-btn {
    padding: var(--space-xs) var(--space-sm);
    background: transparent;
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-primary);
    font-size: var(--font-size-sm);
    cursor: pointer;
  }

  .empty-tracks {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-2xl);
    text-align: center;
    color: var(--p11-text-muted);
  }

  .track-list {
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
    position: relative;
  }

  /* The list holds real DOM focus (roving keyboard nav); the focus ring shows
     on the active row itself (.track-row.active below). */
  .track-list:focus {
    outline: none;
  }

  .track-list-spacer {
    position: relative;
    width: 100%;
  }

  .track-list-window.is-windowed {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .track-header,
  .track-row {
    display: grid;
    /* grid-template-columns is set inline (`gridTemplate`) — the active
       column set, in order, each sized from its stored/fixed width or a
       flexible share. */
    gap: var(--space-md);
    padding: var(--space-sm) var(--space-md);
    align-items: center;
  }

  .col-grip {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--p11-text-muted);
    cursor: grab;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .track-row:hover .col-grip {
    opacity: 0.55;
  }

  .track-row.dragging {
    opacity: 0.4;
    cursor: grabbing;
  }

  /* Kept mounted so the browser's drag session survives an autoscroll that
     carries it out of the virtualised window — taken out of flow so it
     can't displace the rows actually on screen, and hidden since its ghost
     drag image (captured at dragstart) is what the user sees moving. */
  .track-row.keep-alive {
    position: absolute;
    width: 0;
    height: 0;
    padding: 0;
    overflow: hidden;
    opacity: 0;
    pointer-events: none;
  }

  /* Drop indicator: an accent line at the insertion edge (inset box-shadow keeps
     it from shifting layout). */
  .track-row.drop-before {
    box-shadow: inset 0 2px 0 0 var(--p11-accent);
  }

  .track-row.drop-after {
    box-shadow: inset 0 -2px 0 0 var(--p11-accent);
  }

  .track-header {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-bottom: 1px solid var(--p11-border);
    background-color: var(--p11-bg);
    flex-shrink: 0;
    user-select: none;
  }

  /* Reorder drop target, same accent-line language TrackList's own column
     header uses. */
  .col-header-cell.drag-over {
    box-shadow: inset 2px 0 0 0 var(--p11-accent);
  }

  .track-row {
    font-size: var(--font-size-sm);
    border-radius: var(--p11-radius-sm);
    transition: background-color var(--transition-fast);
  }

  .track-row:hover {
    background-color: var(--p11-bg-elevated);
  }

  /* Multi-selected (⌘/Shift-click, or an extended arrow-key range) —
     matches TrackRow's own selected treatment. */
  .track-row.selected,
  .track-row.selected:hover {
    background-color: var(--p11-accent-subtle);
  }

  /* The keyboard cursor. */
  .track-row.active {
    outline: 2px solid var(--p11-border-focus);
    outline-offset: -2px;
  }

  /* Now-playing highlight, matching the shared TrackRow treatment so the
     playlist view is consistent with every other track list. The compound
     `.now-playing .track-title-btn` selector out-specifies `.track-title-btn`
     and its :hover, so the accent title survives hover. */
  .track-row.now-playing {
    background-color: var(--p11-now-playing);
  }

  .track-row.now-playing:hover {
    background-color: var(--p11-accent-subtle);
  }

  .track-row.now-playing .track-title-btn {
    color: var(--p11-accent);
  }

  .col-number {
    color: var(--p11-text-muted);
    text-align: center;
  }

  .col-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-title-btn {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    min-width: 0;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--p11-text-primary);
    text-align: left;
    cursor: pointer;
  }

  .track-title-btn:hover {
    color: var(--p11-accent);
    text-decoration: underline;
  }

  .track-title-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Now-playing indicator, matching TrackRow's equaliser-bars animation. */
  .playing-indicator {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    gap: 2px;
    width: 16px;
    height: 14px;
    flex-shrink: 0;
  }

  .eq-bar {
    width: 2px;
    background-color: var(--p11-accent);
    border-radius: 1px;
    animation: eq-bounce 0.8s ease-in-out infinite;
  }

  .eq-bar:nth-child(1) {
    height: 6px;
    animation-delay: 0s;
  }

  .eq-bar:nth-child(2) {
    height: 10px;
    animation-delay: 0.2s;
  }

  .eq-bar:nth-child(3) {
    height: 5px;
    animation-delay: 0.4s;
  }

  @keyframes eq-bounce {
    0%,
    100% {
      transform: scaleY(0.4);
    }
    50% {
      transform: scaleY(1);
    }
  }

  .paused-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 14px;
    flex-shrink: 0;
    color: var(--p11-accent);
  }

  /* Any non-title column — artist/album/etc. read
     left-aligned in the secondary text colour; a numeric one (time, plays,
     skips, last played) is right-aligned and muted, matching TrackList. */
  .col-cell {
    color: var(--p11-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-cell.align-right {
    color: var(--p11-text-muted);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .col-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .track-row:hover .col-actions,
  .track-row:focus-within .col-actions {
    opacity: 1;
  }
</style>
