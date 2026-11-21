<script lang="ts">
  import { getUiState, setSearchQuery, clearSearch } from '#lib/stores/ui.svelte.js';
  import Search from '@lucide/svelte/icons/search';
  import X from '@lucide/svelte/icons/x';

  interface Props {
    onSearch?: (query: string, activate?: boolean) => void;
    debounceMs?: number;
  }

  let { onSearch, debounceMs = 300 }: Props = $props();

  let inputElement: HTMLInputElement | undefined = $state();
  let debounceTimer: ReturnType<typeof setTimeout> | undefined = $state();
  let isExpanded = $state(false);

  function handleInput(event: Event): void {
    const target = event.target as HTMLInputElement;
    const value = target.value;
    setSearchQuery(value);

    // Clear existing timer
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }

    // Debounce the search callback
    debounceTimer = setTimeout(() => {
      onSearch?.(value);
    }, debounceMs);
  }

  function handleSubmit(event: Event): void {
    event.preventDefault();
    // Immediate search on Enter, cancel pending debounce. `activate` jumps to
    // the top result once these results land (Apple Music/Spotify convention
    // — Return doesn't just resubmit the query, it opens/plays the best match).
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    onSearch?.(getUiState().searchQuery, true);
    // Close popup after search
    isExpanded = false;
  }

  function handleClear(): void {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    clearSearch();
    onSearch?.('');
    inputElement?.focus();
  }

  function handleToggle(): void {
    isExpanded = !isExpanded;
    if (isExpanded) {
      // Focus input after a small delay to allow the popup to render
      setTimeout(() => inputElement?.focus(), 50);
    }
  }

  function handleKeyDown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      if (getUiState().searchQuery) {
        handleClear();
      } else if (isExpanded) {
        isExpanded = false;
      }
    }
  }

  // Close popup when clicking outside
  function handleClickOutside(event: MouseEvent): void {
    const target = event.target as HTMLElement;
    if (!target.closest('.search-container')) {
      isExpanded = false;
    }
  }

  $effect(() => {
    if (isExpanded) {
      document.addEventListener('click', handleClickOutside);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  });

  // ⌘/Ctrl+F (the global shortcut layer, globalShortcuts.ts) opens the popup
  // and focuses the input.
  $effect(() => {
    const onFocusSearch = () => {
      isExpanded = true;
      setTimeout(() => inputElement?.focus(), 50);
    };
    window.addEventListener('bragi-focus-search', onFocusSearch);
    return () => window.removeEventListener('bragi-focus-search', onFocusSearch);
  });
</script>

<div class="search-container">
  <button
    class="search-toggle-btn"
    onclick={handleToggle}
    aria-label="Search library"
    aria-expanded={isExpanded}
    title="Search library"
  >
    <Search size={18} />
  </button>

  {#if isExpanded}
    <div class="search-popup" class:visible={isExpanded}>
      <form class="search-bar" onsubmit={handleSubmit}>
        <span class="search-icon">
          <Search size={14} />
        </span>
        <input
          bind:this={inputElement}
          type="search"
          class="search-input"
          placeholder="Search library..."
          value={getUiState().searchQuery}
          oninput={handleInput}
          onkeydown={handleKeyDown}
          aria-label="Search library"
        />
        {#if getUiState().searchQuery}
          <button type="button" class="clear-btn" onclick={handleClear} aria-label="Clear search">
            <X size={12} />
          </button>
        {/if}
      </form>
    </div>
  {/if}
</div>

<style>
  .search-container {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-toggle-btn {
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
    transition:
      color var(--transition-fast),
      background-color var(--transition-fast);
  }

  .search-toggle-btn:hover {
    color: var(--p11-text-primary);
    background-color: var(--p11-bg-hover);
  }

  .search-popup {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    width: 280px;
    z-index: var(--z-dropdown);
  }

  .search-popup:not(.visible) {
    display: none;
  }

  .search-popup.visible {
    display: block;
    animation: searchFadeIn var(--transition-fast) ease-out;
  }

  @keyframes searchFadeIn {
    from {
      opacity: 0;
      transform: translateY(-8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .search-bar {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    height: 36px;
    padding: 0 var(--space-sm) 0 var(--space-md);
    background-color: var(--p11-bg-elevated);
    border-radius: var(--p11-radius-md);
    border: 1px solid var(--p11-border);
    box-shadow: var(--p11-shadow-md);
    transition:
      border-color var(--transition-fast),
      background-color var(--transition-fast);
  }

  .search-bar:focus-within {
    border-color: var(--p11-accent);
    background-color: var(--p11-bg-surface);
  }

  .search-icon {
    display: flex;
    align-items: center;
    color: var(--p11-text-muted);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    height: 100%;
    background: transparent;
    border: none;
    outline: none;
    font-size: var(--font-size-md);
    font-family: var(--font-family);
    color: var(--p11-text-primary);
  }

  .search-input::placeholder {
    color: var(--p11-text-muted);
  }

  /* Hide native search cancel button */
  .search-input::-webkit-search-cancel-button {
    display: none;
  }

  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background-color: rgb(var(--p11-overlay-rgb) / 0.1);
    border: none;
    border-radius: var(--radius-full);
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    cursor: pointer;
    transition:
      background-color var(--transition-fast),
      color var(--transition-fast);
    flex-shrink: 0;
  }

  .clear-btn:hover {
    background-color: rgb(var(--p11-overlay-rgb) / 0.2);
    color: var(--p11-text-primary);
  }

  :root:not(.dark) .clear-btn {
    background-color: rgb(var(--p11-shade-rgb) / 0.1);
  }

  :root:not(.dark) .clear-btn:hover {
    background-color: rgb(var(--p11-shade-rgb) / 0.2);
  }
</style>
