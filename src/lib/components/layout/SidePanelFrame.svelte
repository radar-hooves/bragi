<script lang="ts">
  import type { Component, Snippet } from 'svelte';
  import X from '@lucide/svelte/icons/x';

  interface Props {
    title: string;
    icon: Component;
    closeLabel: string;
    onClose: () => void;
    ariaLabel?: string;
    headerActions?: Snippet;
    children: Snippet;
  }

  let { title, icon: Icon, closeLabel, onClose, ariaLabel, headerActions, children }: Props = $props();
</script>

<aside class="side-panel" aria-label={ariaLabel}>
  <header class="side-panel-header">
    <div class="side-panel-title">
      <Icon size={16} />
      <span>{title}</span>
    </div>
    <div class="side-panel-header-actions">
      {#if headerActions}
        {@render headerActions()}
      {/if}
      <button class="close-btn" onclick={onClose} aria-label={closeLabel} title={closeLabel}>
        <X size={18} />
      </button>
    </div>
  </header>

  <div class="side-panel-body">
    {@render children()}
  </div>
</aside>

<style>
  .side-panel {
    display: flex;
    flex-direction: column;
    width: 340px;
    flex-shrink: 0;
    height: 100%;
    background-color: var(--p11-bg-surface);
    border-left: 1px solid var(--p11-border);
    overflow: hidden;
  }

  .side-panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-md) var(--space-md) var(--space-sm);
    flex-shrink: 0;
    border-bottom: 1px solid var(--p11-border);
  }

  .side-panel-title {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .side-panel-header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-xs);
  }

  .close-btn {
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

  .close-btn:hover {
    color: var(--p11-text-primary);
    background-color: var(--p11-bg-hover);
  }

  .side-panel-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
</style>
