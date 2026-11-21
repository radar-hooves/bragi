<script lang="ts">
  import { getCurrentQueueTrack, getUpcomingQueueEntries } from '#lib/stores/queue.svelte.js';
  import { setUpNextVisible } from '#lib/stores/ui.svelte.js';
  import { removeFromQueue, reorderQueue, jumpToQueueIndex, clearQueue } from '#lib/services/tauri.js';
  import { formatDuration } from '#lib/components/library/trackColumns.js';
  import { buildUpNextRows, type UpNextRow } from '#lib/utils/upNextRows.js';
  import { createVirtualList } from '#lib/utils/virtualList.svelte.js';
  import { createSelection } from '#lib/utils/selection.svelte.js';
  import { createListKeyboard } from '#lib/utils/listKeyboard.js';
  import { createReorderDrag } from '#lib/utils/reorderDrag.svelte.js';
  import { EmptyState } from '#lib/components/common/index.js';
  import SidePanelFrame from './SidePanelFrame.svelte';
  import { toast } from 'svelte-sonner';
  import ListMusic from '@lucide/svelte/icons/list-music';
  import GripVertical from '@lucide/svelte/icons/grip-vertical';

  const currentTrack = $derived(getCurrentQueueTrack());
  const entries = $derived(getUpcomingQueueEntries());
  const rows = $derived(buildUpNextRows(entries));

  // Uniform row height across headers and tracks — required by the shared
  // virtualiser, which windows on a single fixed stride.
  const ROW_HEIGHT = 36;
  let viewportElement: HTMLDivElement | undefined = $state();
  let innerElement: HTMLDivElement | undefined = $state();

  const virtual = createVirtualList({
    count: () => rows.length,
    rowHeight: ROW_HEIGHT,
    overscan: 10,
  });

  let visibleRows = $derived(rows.slice(virtual.start, virtual.end));

  $effect(() => virtual.attachViewport(viewportElement));
  $effect(() => {
    rows.length;
    virtual.refresh();
  });

  function isEntryRow(row: UpNextRow): row is Extract<UpNextRow, { kind: 'entry' }> {
    return row.kind === 'entry';
  }

  // Selection/keyboard operate over the entry rows only, keyed by each
  // entry's absolute queue index — a track can legitimately repeat in the
  // queue, so `jellyfinId` is not a safe id here (PlaylistView faces the same
  // problem and keys on its own per-instance id for the same reason).
  const entryRows = $derived(rows.filter(isEntryRow));
  const entryIds = $derived(entryRows.map((r) => r.key));
  const entryByIndexId = $derived(new Map(entryRows.map((r) => [r.key, r.entry])));

  const selection = createSelection(() => entryIds);

  async function handleJump(index: number): Promise<void> {
    try {
      await jumpToQueueIndex(index);
    } catch (e) {
      console.error('Failed to jump to queue index:', e);
      toast.error('Could not play that track');
    }
  }

  async function handleRemove(ids: string[]): Promise<void> {
    // Highest index first — removing a lower index would shift every
    // subsequent absolute index out from under the rest of the batch.
    const indices = ids.map(Number).sort((a, b) => b - a);
    for (const index of indices) {
      try {
        await removeFromQueue(index);
      } catch (e) {
        console.error('Failed to remove from queue:', e);
        toast.error('Could not remove that track');
        return;
      }
    }
  }

  async function handleClear(): Promise<void> {
    try {
      await clearQueue();
    } catch (e) {
      console.error('Failed to clear queue:', e);
      toast.error('Could not clear the queue');
    }
  }

  function revealActiveRow(entryIndex: number): void {
    const id = entryIds[entryIndex];
    if (id === undefined) return;
    const rowIndex = rows.findIndex((r) => r.kind === 'entry' && r.key === id);
    if (rowIndex < 0) return;
    if (rowIndex >= virtual.start && rowIndex < virtual.end) {
      viewportElement?.querySelector(`[data-entry-id="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'nearest' });
    } else {
      virtual.scrollToIndex(rowIndex, 'center');
    }
  }

  const listKeyboard = createListKeyboard({
    getIds: () => entryIds,
    selection,
    getLabel: (id) => entryByIndexId.get(id)?.track.name ?? '',
    onActivate: (ids) => {
      const first = ids[0];
      if (first !== undefined) handleJump(Number(first));
    },
    onDelete: (ids) => handleRemove(ids),
    revealIndex: revealActiveRow,
  });

  function handleRowClick(id: string, event: MouseEvent): void {
    selection.click(id, { shiftKey: event.shiftKey, metaKey: event.metaKey, ctrlKey: event.ctrlKey });
  }

  // Drag-to-reorder (`reorderDrag.svelte.ts`), operating
  // directly in ABSOLUTE queue indices: `entries` is already a contiguous,
  // ascending slice of the backend queue, so a local drag position IS the
  // absolute index — no translation layer needed.
  const lastEntryIndex = $derived(entries[entries.length - 1]?.index ?? -1);

  async function performReorder(from: number, to: number): Promise<void> {
    try {
      await reorderQueue(from, to);
    } catch (e) {
      console.error('Failed to reorder queue:', e);
      toast.error('Could not reorder the queue');
    }
  }

  const reorder = createReorderDrag({
    onReorder: (from, to) => void performReorder(from, to),
  });

  function handleClose(): void {
    setUpNextVisible(false);
  }
</script>

<SidePanelFrame title="Up Next" icon={ListMusic} closeLabel="Close Up Next" onClose={handleClose}>
  {#snippet headerActions()}
    <button
      class="clear-btn"
      onclick={handleClear}
      disabled={entries.length === 0}
      aria-label="Clear queue"
      title="Clear queue"
    >
      Clear
    </button>
  {/snippet}

  {#if currentTrack}
    <div class="now-playing-row">
      <div class="now-playing-label">Now Playing</div>
      <div class="track-line">
        <span class="track-name">{currentTrack.name}</span>
        <span class="track-artist">{currentTrack.artistName}</span>
      </div>
    </div>
  {/if}

  {#if rows.length === 0}
    <EmptyState title="Nothing queued" description="Play Next or Add to Queue puts tracks here." />
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="up-next-viewport"
      bind:this={viewportElement}
      onscroll={virtual.handleScroll}
      tabindex="0"
      role="listbox"
      aria-label="Up Next queue"
      aria-activedescendant={selection.activeId ? `up-next-row-${selection.activeId}` : undefined}
      onkeydown={listKeyboard.handleKeyDown}
    >
      <div
        class="up-next-spacer"
        style:height={virtual.windowing ? `${virtual.totalSize}px` : undefined}
      >
        <div
          class="up-next-window"
          bind:this={innerElement}
          style:transform={virtual.windowing ? `translateY(${virtual.padStart}px)` : undefined}
        >
          {#each visibleRows as row (row.key)}
            {#if row.kind === 'header'}
              <div class="section-header" role="presentation">{row.label}</div>
            {:else}
              {@const entry = row.entry}
              <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
              <!-- svelte-ignore a11y_interactive_supports_focus -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div
                id={`up-next-row-${row.key}`}
                data-entry-id={row.key}
                class="entry-row"
                class:selected={selection.isSelected(row.key)}
                class:active={selection.activeId === row.key}
                class:drop-before={reorder.dropIndex === entry.index}
                class:drop-after={reorder.dropIndex === entry.index + 1 && entry.index === lastEntryIndex}
                tabindex="-1"
                role="option"
                aria-selected={selection.isSelected(row.key)}
                draggable="true"
                ondragstart={(e) => reorder.onDragStart(e, entry.index)}
                ondragover={(e) => reorder.onDragOver(e, entry.index)}
                ondrop={reorder.onDrop}
                ondragend={reorder.onDragEnd}
                onclick={(e) => handleRowClick(row.key, e)}
                ondblclick={() => handleJump(entry.index)}
              >
                <span class="grip" aria-hidden="true" title="Drag to reorder">
                  <GripVertical size={14} />
                </span>
                <span class="track-line">
                  <span class="track-name">{entry.track.name}</span>
                  <span class="track-artist">{entry.track.artistName}</span>
                </span>
                <span class="track-duration">{formatDuration(entry.track.durationMs)}</span>
              </div>
            {/if}
          {/each}
        </div>
      </div>
    </div>
  {/if}
</SidePanelFrame>

<style>
  .clear-btn {
    padding: var(--space-2xs) var(--space-sm);
    background: transparent;
    border: none;
    border-radius: var(--p11-radius-sm);
    color: var(--p11-text-muted);
    font-size: var(--font-size-xs);
    cursor: pointer;
    transition: color var(--transition-fast), background-color var(--transition-fast);
  }

  .clear-btn:hover:not(:disabled) {
    color: var(--p11-text-primary);
    background-color: var(--p11-bg-hover);
  }

  .clear-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .now-playing-row {
    flex-shrink: 0;
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-now-playing);
    border-bottom: 1px solid var(--p11-border);
  }

  .now-playing-label {
    font-size: var(--font-size-2xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-accent);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: 2px;
  }

  .up-next-viewport {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .up-next-viewport:focus {
    outline: none;
  }

  .up-next-spacer {
    position: relative;
  }

  .up-next-window {
    display: flex;
    flex-direction: column;
  }

  .section-header {
    flex-shrink: 0;
    height: 36px;
    display: flex;
    align-items: center;
    padding: 0 var(--space-md);
    font-size: var(--font-size-2xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .entry-row {
    flex-shrink: 0;
    height: 36px;
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: 0 var(--space-md);
    cursor: default;
    transition: background-color var(--transition-fast);
  }

  .entry-row:hover {
    background-color: var(--p11-bg-hover);
  }

  .entry-row.selected {
    background-color: var(--p11-accent-subtle);
  }

  .entry-row.active {
    box-shadow: inset 0 0 0 1px var(--p11-accent);
  }

  .entry-row.drop-before {
    box-shadow: inset 0 2px 0 0 var(--p11-accent);
  }

  .entry-row.drop-after {
    box-shadow: inset 0 -2px 0 0 var(--p11-accent);
  }

  .grip {
    display: flex;
    align-items: center;
    color: var(--p11-text-muted);
    cursor: grab;
    flex-shrink: 0;
  }

  .track-line {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .track-name {
    font-size: var(--font-size-sm);
    color: var(--p11-text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-artist {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-duration {
    flex-shrink: 0;
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    font-variant-numeric: tabular-nums;
  }
</style>
