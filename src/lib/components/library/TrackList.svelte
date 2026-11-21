<script lang="ts">
  import { untrack } from 'svelte';
  import type { Track } from '#lib/types/models.js';
  import TrackListHeaderMenu from './TrackListHeaderMenu.svelte';
  import TrackRow from './TrackRow.svelte';
  import { type ColumnDef, resolveActiveColumns, compareTracks, computeFitWidth, PINNED_FIRST_COLUMN } from './trackColumns';
  import { getPendingRevealId, clearPendingReveal } from '#lib/utils/revealInLibrary.svelte.js';
  import { queueTrackNext, queueTrackAtEnd } from '#lib/utils/queueActions.js';
  import { createVirtualList } from '#lib/utils/virtualList.svelte.js';
  import { createSelection } from '#lib/utils/selection.svelte.js';
  import { createListKeyboard } from '#lib/utils/listKeyboard.js';
  import { createColumnHeader } from '#lib/utils/columnHeader.svelte.js';

  interface Props {
    tracks: Track[];
    visibleColumns: string[];
    columnWidths: Record<string, number>;
    /** Keys this list's remembered scroll position; views sharing a TrackList (Songs, Recently Played, search) each pass their own. */
    viewKey?: string;
    onTrackClick?: (track: Track) => void;
    onTrackPlay?: (track: Track) => void;
    onColumnsChange?: (columns: string[]) => void;
    onColumnWidthsChange?: (widths: Record<string, number>) => void;
    onAddToPlaylist?: (tracks: Track[]) => void;
  }

  let {
    tracks,
    visibleColumns,
    columnWidths,
    viewKey = 'tracks',
    onTrackClick,
    onTrackPlay,
    onColumnsChange,
    onColumnWidthsChange,
    onAddToPlaylist,
  }: Props = $props();

  type SortDirection = 'asc' | 'desc';
  let sortColumn = $state<string | null>(null);
  let sortDirection = $state<SortDirection | null>(null);
  let menuPosition = $state<{ x: number; y: number } | null>(null);
  let tableElement: HTMLTableElement | undefined = $state();

  // Virtual scrolling: a fixed 40px row height, matching TrackRow's real
  // rendered height, through the shared virtualiser every long list uses.
  const ROW_HEIGHT = 40;
  let viewportElement: HTMLDivElement | undefined = $state();

  const virtual = createVirtualList({
    count: () => sortedTracks.length,
    rowHeight: ROW_HEIGHT,
    overscan: 10,
  });

  let visibleTracks = $derived.by(() => sortedTracks.slice(virtual.start, virtual.end));
  let topSpacerHeight = $derived(virtual.padStart);
  let bottomSpacerHeight = $derived(virtual.padEnd);

  // The narrowest the table may get: each column's set width, a floor for a
  // flexible text column, and the download column. Narrower windows scroll sideways.
  const FLEX_COLUMN_MIN_PX = 160;
  let activeColumns = $derived(resolveActiveColumns(visibleColumns));
  let minTableWidth = $derived(
    activeColumns.reduce((sum, col) => sum + (columnWidths[col.id] || col.fixedWidth || FLEX_COLUMN_MIN_PX), 64)
  );

  // Own the viewport's resize handling and remember its scroll position, keyed
  // by the calling view (Songs, Recently Played, search, ...) so switching
  // between them resumes each one where it was left.
  $effect(() => virtual.attachViewport(viewportElement));
  $effect(() => virtual.bindScrollMemory(viewKey, viewportElement));

  // Recalculate when the track list changes (filter/sort/section switch).
  $effect(() => {
    sortedTracks; // read the dependency
    virtual.refresh();
  });

  let sortedTracks = $derived.by(() => {
    // Track the array ref + sort inputs, but spread/sort the (up to 23k)
    // deeply-reactive tracks under untrack() to avoid Svelte 5's O(n^2)
    // dependency-tracking cliff on the Songs view.
    const list = tracks;
    const col = sortColumn;
    const direction = sortDirection;
    return untrack(() => {
      // No column chosen (including the third click back off a header): show
      // the list in the order its caller already supplies it — library order
      // for Songs, recency for Recently Played, relevance for search. Never
      // re-sort it by disc/track/title, which would hide that order.
      if (!col || !direction) {
        return list;
      }
      const dir = direction === 'asc' ? 1 : -1;
      return [...list].sort((a, b) => compareTracks(a, b, col, dir));
    });
  });

  // One selection model and one list-keyboard handler, over
  // this view's own current order — so range-select and type-to-find always
  // match what's on screen, whichever caller (Songs, Recently Played, search)
  // supplied the tracks. `trackIds`/`trackById` are read under untrack() for
  // the same O(n^2) dependency-tracking reason as `sortedTracks` above.
  const trackIds = $derived.by(() => {
    const list = sortedTracks;
    return untrack(() => list.map((t) => t.jellyfinId));
  });
  const trackById = $derived.by(() => {
    const list = sortedTracks;
    return untrack(() => new Map(list.map((t) => [t.jellyfinId, t])));
  });

  const selection = createSelection(() => trackIds);

  function revealActiveRow(index: number): void {
    if (!viewportElement) return;
    const id = trackIds[index];
    // Still mounted (inside the windowed slice): scrollIntoView gets the exact
    // "nearest edge" behaviour arrow-key nav wants. Scrolled out of the
    // rendered window, jump the virtualiser there directly (the ⌘L-reveal
    // mechanism), same fallback ColumnBrowserColumn uses for its own list.
    if (id && index >= virtual.start && index < virtual.end) {
      viewportElement.querySelector(`[data-track-id="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'nearest' });
    } else {
      virtual.scrollToIndex(index, 'center');
    }
  }

  const listKeyboard = createListKeyboard({
    getIds: () => trackIds,
    selection,
    getLabel: (id) => trackById.get(id)?.name ?? '',
    // Return plays the selection — the same "play this row, queue the rest of
    // the list" mechanism a double-click or the row's own Play button uses,
    // starting from the first selected track (startPlayback.ts).
    onActivate: (ids) => {
      const track = trackById.get(ids[0]!);
      if (track) onTrackPlay?.(track);
    },
    revealIndex: revealActiveRow,
  });

  function handleRowClick(track: Track, event: MouseEvent): void {
    selection.click(track.jellyfinId, { shiftKey: event.shiftKey, metaKey: event.metaKey, ctrlKey: event.ctrlKey });
    onTrackClick?.(track);
  }

  // Right-click/drag on a row that's part of a multi-selection acts on the
  // WHOLE selection, matching Spotify/Apple Music (add-to-playlist, drag and
  // the queue actions all carry the selection, not just the row under the
  // pointer); a row outside the current selection still acts on just itself
  // (`selection.targetIds`).
  function tracksForAction(track: Track): Track[] {
    return selection.targetIds(track.jellyfinId).map((id) => trackById.get(id)).filter((t): t is Track => !!t);
  }

  function handleAddToPlaylist(tracks: Track[]): void {
    const track = tracks[0];
    if (!track) return;
    onAddToPlaylist?.(tracksForAction(track));
  }

  function handleQueueNext(tracks: Track[]): void {
    const track = tracks[0];
    if (!track) return;
    queueTrackNext(tracksForAction(track));
  }

  function handleQueueEnd(tracks: Track[]): void {
    const track = tracks[0];
    if (!track) return;
    queueTrackAtEnd(tracksForAction(track));
  }

  // A pending sort-toggle click must be dropped when it's really the tail end
  // of a resize or a column-reorder drop — see handleHeaderClick below.
  let suppressNextHeaderClick = false;

  // Fit-to-content measurement for double-click-to-fit: a canvas context using this table's own cell font, so the
  // measured width matches what's actually rendered.
  let measureCanvas: HTMLCanvasElement | null = null;
  function getMeasureContext(): CanvasRenderingContext2D | null {
    if (!measureCanvas) measureCanvas = document.createElement('canvas');
    const ctx = measureCanvas.getContext('2d');
    if (ctx && tableElement) {
      const sampleCell = tableElement.querySelector<HTMLElement>('tbody td, th');
      if (sampleCell) ctx.font = getComputedStyle(sampleCell).font;
    }
    return ctx;
  }

  function measureFitWidth(col: ColumnDef): number {
    const ctx = getMeasureContext();
    if (!ctx) return col.minWidth;
    return computeFitWidth(sortedTracks.map((t) => col.render(t)), (text) => ctx.measureText(text).width, col.minWidth);
  }

  // Resize (drag + double-click-to-fit) and drag-to-reorder: one shared
  // mechanism (`columnHeader.svelte.ts`) TrackList and PlaylistView both
  // drive. The identity column (Title) is pinned first, like Finder's Name
  // column and Apple Music's Song Name column — never draggable, never a
  // valid drop target.
  const columnHeader = createColumnHeader({
    getColumns: () => activeColumns,
    getWidths: () => columnWidths,
    onWidthsChange: (widths) => onColumnWidthsChange?.(widths),
    onOrderChange: (order) => {
      suppressNextHeaderClick = true;
      onColumnsChange?.(order);
    },
    pinnedFirstColumn: PINNED_FIRST_COLUMN,
    measureFitWidth,
  });

  // Header click toggles sort — TrackList's own behaviour, layered on top of
  // the shared resize/reorder mechanism: a click that's really the tail end
  // of a resize-drag release or a reorder drop must never also toggle sort.
  function handleHeaderClick(event: MouseEvent): void {
    if (columnHeader.resizing) return;
    if (suppressNextHeaderClick) { suppressNextHeaderClick = false; return; }
    if (columnHeader.isResizeEdge(event)) return;
    const th = (event.target as HTMLElement).closest<HTMLElement>('th[data-col]');
    if (!th) return;
    const colId = th.dataset.col!;
    if (sortColumn !== colId) { sortColumn = colId; sortDirection = 'asc'; }
    else if (sortDirection === 'asc') { sortDirection = 'desc'; }
    else { sortColumn = null; sortDirection = null; }
  }

  // Column-visibility menu: only from the header itself. A row owns its own
  // right-click (TrackContextMenu, via TrackRow) — without this guard the
  // container's own oncontextmenu below fires for a row too, since the event
  // bubbles up past it, and the WRONG menu (column toggles) would win.
  function handleHeaderContextMenu(event: MouseEvent): void {
    if (!(event.target as HTMLElement).closest('thead')) return;
    event.preventDefault();
    menuPosition = { x: event.clientX, y: event.clientY };
  }

  function handleToggleColumn(columnId: string): void {
    const current = [...visibleColumns];
    const idx = current.indexOf(columnId);
    if (idx >= 0) current.splice(idx, 1);
    else current.push(columnId);
    onColumnsChange?.(current);
  }

  // "Show in Library" reveal. The list is virtualised, so the row only
  // exists once the filtered query lands — and a reveal from another section
  // mounts this component in the same tick, so the request is read from state
  // rather than an event that would fire before this listener existed.
  $effect(() => {
    const id = getPendingRevealId();
    const list = sortedTracks;
    if (!id || !viewportElement) return;
    // Scan under untrack() — per-property reads across 23k reactive tracks hit
    // Svelte 5's O(n^2) dependency-tracking cliff (see sortedTracks).
    const index = untrack(() => list.findIndex((t) => t.jellyfinId === id));
    if (index < 0) return;
    clearPendingReveal();
    selection.selectOnly(id);
    virtual.scrollToIndex(index, 'center');
  });

  function handleDragStart(event: DragEvent, track: Track): void {
    if (!event.dataTransfer) return;
    const dragTracks = tracksForAction(track);
    const trackIds = dragTracks.map((t) => t.jellyfinId);
    event.dataTransfer.setData('application/x-bragi-playlist', JSON.stringify({ type: 'tracks', trackIds }));
    event.dataTransfer.setData(
      'text/plain',
      dragTracks.length === 1 ? `Track: ${track.name}` : `${dragTracks.length} tracks`
    );
    event.dataTransfer.effectAllowed = 'copyMove';
  }
</script>

{#if tracks.length === 0}
  <div class="empty-state">
    <p class="empty-title">No songs found</p>
    <p class="empty-description">Try adjusting your filters or search criteria.</p>
  </div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="track-list-container" role="region" oncontextmenu={handleHeaderContextMenu}>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="track-table-viewport"
      bind:this={viewportElement}
      onscroll={virtual.handleScroll}
      tabindex="0"
      onkeydown={listKeyboard.handleKeyDown}
      aria-activedescendant={selection.activeId ? `track-row-${selection.activeId}` : undefined}
    >
      <table class="track-table" style:min-width="{minTableWidth}px" bind:this={tableElement}>
        <colgroup>
          {#each activeColumns as col (col.id)}<col style:width={columnHeader.widthFor(col)} />{/each}
          <col style="width: 64px" />
        </colgroup>
        <thead>
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <tr
            onmousedown={columnHeader.handleMouseDown}
            onmousemove={columnHeader.handleMouseMove}
            onclick={handleHeaderClick}
            ondblclick={columnHeader.handleDoubleClick}
          >
            {#each activeColumns as col (col.id)}
              <th
                class="col-header"
                class:align-right={col.align === 'right'}
                class:align-center={col.align === 'center'}
                class:sorted={sortColumn === col.id}
                class:drag-over={columnHeader.dragOverColId === col.id}
                data-col={col.id}
                draggable={columnHeader.isDraggable(col.id)}
                ondragstart={(e) => columnHeader.handleDragStart(e, col.id)}
                ondragover={(e) => columnHeader.handleDragOver(e, col.id)}
                ondrop={(e) => columnHeader.handleDrop(e, col.id)}
                ondragend={columnHeader.handleDragEnd}
              >
                <span class="col-header-content">
                  <span class="col-header-label">{col.label.toUpperCase()}</span>
                  {#if sortColumn === col.id}<span class="sort-indicator">{sortDirection === 'asc' ? '▲' : '▼'}</span>{/if}
                </span>
              </th>
            {/each}
            <th class="col-download-header"></th>
          </tr>
        </thead>
        <tbody>
          {#if topSpacerHeight > 0}
            <tr class="spacer-row" aria-hidden="true">
              <td colspan={activeColumns.length + 1} style:height="{topSpacerHeight}px"></td>
            </tr>
          {/if}
          {#each visibleTracks as track, i (track.jellyfinId)}
            <TrackRow
              {track}
              {activeColumns}
              striped={(virtual.start + i) % 2 === 1}
              selected={selection.isSelected(track.jellyfinId)}
              active={selection.activeId === track.jellyfinId}
              showAddToPlaylist={!!onAddToPlaylist}
              onClick={handleRowClick}
              onDoubleClick={onTrackPlay}
              onDragStart={handleDragStart}
              onPlay={onTrackPlay}
              onQueueNext={handleQueueNext}
              onQueueEnd={handleQueueEnd}
              onAddToPlaylist={onAddToPlaylist ? handleAddToPlaylist : undefined}
            />
          {/each}
          {#if bottomSpacerHeight > 0}
            <tr class="spacer-row" aria-hidden="true">
              <td colspan={activeColumns.length + 1} style:height="{bottomSpacerHeight}px"></td>
            </tr>
          {/if}
        </tbody>
      </table>
    </div>
  </div>
{/if}

{#if menuPosition}
  <TrackListHeaderMenu {visibleColumns} onToggleColumn={handleToggleColumn} position={menuPosition} onClose={() => (menuPosition = null)} />
{/if}

<style>
  .track-list-container { display: flex; flex-direction: column; width: 100%; height: 100%; }
  .track-table { width: 100%; table-layout: fixed; border-collapse: collapse; border-spacing: 0; }
  .track-table-viewport { flex: 1; min-height: 0; overflow: auto; overscroll-behavior: contain; position: relative; }
  /* The viewport holds real DOM focus (roving keyboard nav); the focus ring
     shows on the active row itself (TrackRow's own :global(.active) rule). */
  .track-table-viewport:focus { outline: none; }
  /* One table for header + body: a sticky header shares the viewport's own
     scrollbar gutter with tbody, so their column widths can never drift apart
     the way two independently-laid-out tables did. Sticky lives on
     each header cell, not on `thead` itself: WKWebView's support for
     `position: sticky` on `table-header-group` display (thead) is unreliable,
     while sticky on `table-cell` display (th) is universal — and a cell needs
     its own background regardless, since sticky positioning doesn't inherit
     one from an ancestor that's about to scroll out from under it. */
  .spacer-row { pointer-events: none; }
  .spacer-row td { padding: 0; border: 0; line-height: 0; }

  .col-header {
    position: sticky;
    top: 0;
    z-index: var(--z-sticky);
    background-color: var(--p11-bg-primary);
    padding: var(--space-sm) var(--space-md);
    font-size: var(--font-size-2xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    border-bottom: 1px solid var(--p11-border-subtle);
    user-select: none;
  }

  .col-header-content { display: inline-flex; align-items: center; gap: 4px; max-width: 100%; overflow: hidden; }
  .col-header-label { overflow: hidden; text-overflow: ellipsis; }
  .sort-indicator { font-size: var(--font-size-2xs); line-height: 1; color: var(--p11-accent); flex-shrink: 0; }
  .col-header.sorted { color: var(--p11-text-primary); }

  .col-header::after {
    content: '';
    position: absolute;
    right: 0;
    top: 25%;
    height: 50%;
    width: 1px;
    background-color: rgb(var(--p11-overlay-rgb) / 0.08);
    transition: background-color var(--transition-fast);
  }

  .col-header:hover::after { background-color: rgb(var(--p11-overlay-rgb) / 0.2); }

  /* Reorder drop target, same accent-line language as PlaylistView's
     drag-to-reorder indicator. */
  .col-header.drag-over { box-shadow: inset 2px 0 0 0 var(--p11-accent); }

  .col-download-header {
    position: sticky;
    top: 0;
    z-index: var(--z-sticky);
    background-color: var(--p11-bg-primary);
    width: 64px;
    padding: 0;
  }

  .empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: var(--space-3xl); text-align: center; }
  .empty-title { font-size: var(--font-size-lg); font-weight: var(--font-weight-medium); color: var(--p11-text-primary); margin: 0 0 var(--space-sm); }
  .empty-description { font-size: var(--font-size-md); color: var(--p11-text-secondary); margin: 0; }
  .align-right { text-align: right; }
  .align-center { text-align: center; }
</style>
