<script lang="ts">
  import { allColumns, DEFAULT_TOGGLEABLE_COLUMNS } from './trackColumns';

  interface Props {
    visibleColumns: string[];
    onToggleColumn: (columnId: string) => void;
    position: { x: number; y: number };
    onClose: () => void;
    /** Which columns this list lets the user toggle — TrackList offers every
     * column; PlaylistView offers only what a playlist row can actually
     * supply (`PLAYLIST_TOGGLEABLE_COLUMNS`). */
    toggleableColumns?: string[];
  }

  let { visibleColumns, onToggleColumn, position, onClose, toggleableColumns = DEFAULT_TOGGLEABLE_COLUMNS }: Props =
    $props();

  function handleBackdropClick(event: MouseEvent): void {
    if (event.target === event.currentTarget) {
      onClose();
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      onClose();
    }
  }

  function handleToggle(columnId: string): void {
    onToggleColumn(columnId);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="menu-backdrop" onclick={handleBackdropClick}>
  <div class="menu" role="menu" style="left: {position.x}px; top: {position.y}px;">
    <div class="menu-header">Columns</div>
    {#each toggleableColumns as colId (colId)}
      <label class="menu-item">
        <input
          type="checkbox"
          checked={visibleColumns.includes(colId)}
          onchange={() => handleToggle(colId)}
        />
        <span class="menu-label">{allColumns[colId]?.label}</span>
      </label>
    {/each}
  </div>
</div>

<style>
  .menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
  }

  .menu {
    position: fixed;
    min-width: 160px;
    background: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    box-shadow: var(--p11-shadow-lg);
    padding: var(--space-xs) 0;
    z-index: 1001;
  }

  .menu-header {
    padding: var(--space-xs) var(--space-md);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-bottom: 1px solid var(--p11-border);
    margin-bottom: var(--space-xs);
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-md);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .menu-item:hover {
    background-color: rgb(var(--p11-overlay-rgb) / 0.06);
  }

  .menu-item input[type='checkbox'] {
    accent-color: var(--p11-accent);
    width: 14px;
    height: 14px;
    cursor: pointer;
  }

  .menu-label {
    font-size: var(--font-size-sm);
    color: var(--p11-text-primary);
  }
</style>
