<script lang="ts">
  import { createVirtualList } from '#lib/utils/virtualList.svelte.js';

  interface ColumnItem {
    id: string;
    label: string;
    sublabel?: string;
    selected: boolean;
  }

  /** Event with modifier keys for multi-select detection */
  interface ModifierKeyEvent {
    metaKey: boolean;
    ctrlKey: boolean;
  }

  interface Props {
    title: string;
    items: ColumnItem[];
    allCount: number;
    allSelected: boolean;
    onItemClick: (id: string, event: ModifierKeyEvent) => void;
    onItemDoubleClick?: (id: string) => void;
    onAllClick: () => void;
    showSublabel?: boolean;
  }

  let {
    title,
    items,
    allCount,
    allSelected,
    onItemClick,
    onItemDoubleClick,
    onAllClick,
    showSublabel = false,
  }: Props = $props();

  // Generate unique ID prefix for this column instance (for aria-activedescendant)
  // Using a random ID ensures uniqueness across multiple column instances
  const columnId = `column-${Math.random().toString(36).slice(2, 11)}`;

  // Keyboard navigation state
  let focusedIndex = $state(-1);
  let listElement: HTMLDivElement | undefined = $state();
  let allBlockElement: HTMLDivElement | undefined = $state();
  let innerElement: HTMLDivElement | undefined = $state();

  /** Get the element ID for a given index */
  function getOptionId(index: number): string {
    return `${columnId}-option-${index}`;
  }

  /** Get the aria-activedescendant value based on focused index */
  const activeDescendant = $derived(focusedIndex >= 0 ? getOptionId(focusedIndex) : undefined);

  // This column can hold a few thousand artists or albums, so it routes
  // through the shared virtualiser. The "All" row + separator are
  // always rendered above the windowed items, in the same scroll container —
  // leadingOffset keeps the row math and scrollToIndex() aware of that.
  const virtual = createVirtualList({
    count: () => items.length,
    estimateRowHeight: () => 28,
    overscan: 8,
    leadingOffset: () => allBlockElement?.offsetHeight ?? 0,
  });

  let visibleItems = $derived(items.slice(virtual.start, virtual.end));

  $effect(() => virtual.attachViewport(listElement));
  $effect(() => virtual.bindScrollMemory(`column:${title}`, listElement));

  $effect(() => {
    items.length;
    virtual.refresh();
  });

  $effect(() => {
    void virtual.windowing;
    void virtual.end;
    virtual.remeasure(innerElement);
  });

  function handleKeyDown(event: KeyboardEvent): void {
    const totalItems = items.length + 1; // +1 for "All" option

    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        focusedIndex = Math.min(focusedIndex + 1, totalItems - 1);
        scrollToFocused();
        break;
      case 'ArrowUp':
        event.preventDefault();
        focusedIndex = Math.max(focusedIndex - 1, 0);
        scrollToFocused();
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        if (focusedIndex === 0) {
          onAllClick();
        } else if (focusedIndex > 0) {
          const item = items[focusedIndex - 1];
          if (item) {
            onItemClick(item.id, { metaKey: event.metaKey, ctrlKey: event.ctrlKey });
          }
        }
        break;
      case 'Home':
        event.preventDefault();
        focusedIndex = 0;
        scrollToFocused();
        break;
      case 'End':
        event.preventDefault();
        focusedIndex = totalItems - 1;
        scrollToFocused();
        break;
    }
  }

  function scrollToFocused(): void {
    if (!listElement) return;
    if (focusedIndex === 0) {
      listElement.scrollTop = 0; // "All" is always the very first row
      return;
    }
    const itemIndex = focusedIndex - 1;
    // Still mounted (inside the windowed slice): scrollIntoView gets the exact
    // "nearest edge" behaviour arrow-key nav wants. Scrolled out of the
    // rendered window, there is no element to query — jump the virtualiser
    // there directly (the ⌘L-reveal mechanism, not a DOM query).
    if (itemIndex >= virtual.start && itemIndex < virtual.end) {
      listElement.querySelector(`[data-index="${focusedIndex}"]`)?.scrollIntoView({ block: 'nearest' });
    } else {
      virtual.scrollToIndex(itemIndex, 'center');
    }
  }

  // Type-to-search
  let searchBuffer = $state('');
  let searchTimeout: ReturnType<typeof setTimeout> | undefined;

  // Cleanup timeout when component is destroyed
  $effect(() => {
    return () => {
      if (searchTimeout) {
        clearTimeout(searchTimeout);
      }
    };
  });

  function handleKeyPress(event: KeyboardEvent): void {
    // Only handle printable characters
    if (event.key.length !== 1) return;

    if (searchTimeout) {
      clearTimeout(searchTimeout);
    }
    searchBuffer += event.key.toLowerCase();

    // Find first matching item
    const matchIndex = items.findIndex((item) => item.label.toLowerCase().startsWith(searchBuffer));

    if (matchIndex !== -1) {
      focusedIndex = matchIndex + 1; // +1 for "All" option
      scrollToFocused();
    }

    // Clear buffer after 500ms
    searchTimeout = setTimeout(() => {
      searchBuffer = '';
    }, 500);
  }
</script>

<div class="column-content">
  <div class="column-header">
    <span class="column-title">{title}</span>
    <span class="column-count">({items.length})</span>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    bind:this={listElement}
    class="column-list"
    role="listbox"
    aria-label={title}
    aria-activedescendant={activeDescendant}
    tabindex="0"
    onkeydown={handleKeyDown}
    onkeypress={handleKeyPress}
    onscroll={virtual.handleScroll}
  >
    <div bind:this={allBlockElement}>
      <button
        id={getOptionId(0)}
        class="column-item all-item"
        class:selected={allSelected}
        class:focused={focusedIndex === 0}
        data-index="0"
        type="button"
        role="option"
        aria-selected={allSelected}
        onclick={onAllClick}
        onfocus={() => (focusedIndex = 0)}
      >
        All ({allCount})
      </button>

      <div class="column-separator" role="presentation"></div>
    </div>

    <div class="column-items-spacer" style:height={virtual.windowing ? `${virtual.totalSize}px` : undefined}>
      <div
        class="column-items-window"
        class:is-windowed={virtual.windowing}
        bind:this={innerElement}
        style:transform={virtual.windowing ? `translateY(${virtual.padStart}px)` : undefined}
      >
        {#each visibleItems as item, i (item.id)}
          {@const index = virtual.start + i}
          <button
            id={getOptionId(index + 1)}
            class="column-item"
            class:selected={item.selected}
            class:focused={focusedIndex === index + 1}
            data-index={index + 1}
            type="button"
            role="option"
            aria-selected={item.selected}
            onclick={(e) => onItemClick(item.id, e)}
            ondblclick={() => onItemDoubleClick?.(item.id)}
            onfocus={() => (focusedIndex = index + 1)}
          >
            <span class="item-label">{item.label}</span>
            {#if showSublabel && item.sublabel}
              <span class="item-sublabel">{item.sublabel}</span>
            {/if}
            {#if item.selected}
              <span class="selection-indicator" aria-hidden="true">&#9664;</span>
            {/if}
          </button>
        {/each}
      </div>
    </div>
  </div>
</div>

<style>
  .column-content {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .column-header {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-bg-elevated);
    border-bottom: 1px solid var(--p11-border);
    flex-shrink: 0;
  }

  .column-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--p11-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .column-count {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-normal);
    color: var(--p11-text-muted);
  }

  .column-list {
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: var(--space-xs) 0;
    outline: none;
    position: relative;
  }

  .column-list:focus-visible {
    outline: none;
  }

  .column-items-spacer {
    position: relative;
    width: 100%;
  }

  .column-items-window.is-windowed {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .column-separator {
    height: 1px;
    background-color: var(--p11-border);
    margin: var(--space-xs) var(--space-md);
  }

  .column-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    width: 100%;
    padding: var(--space-xs) var(--space-md);
    background: none;
    border: none;
    text-align: left;
    font-family: var(--font-family);
    font-size: var(--font-size-sm);
    color: var(--p11-text-primary);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .column-item:hover {
    background-color: var(--p11-bg-hover);
  }

  /* Selected as a track row is: the accent's tint under the normal ink. A solid
     accent fill put white text under 3:1 on the lighter accents. */
  .column-item.selected,
  .column-item.selected:hover {
    background-color: var(--p11-accent-subtle);
  }

  .column-item.focused {
    outline: 2px solid var(--p11-border-focus);
    outline-offset: -2px;
  }

  .all-item {
    font-weight: var(--font-weight-medium);
  }

  .item-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-sublabel {
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 120px;
  }

  .column-item.selected .item-sublabel {
    color: var(--p11-text-secondary);
  }

  .selection-indicator {
    font-size: var(--font-size-xs);
    color: var(--p11-accent);
  }
</style>
