<script lang="ts">
  import type { Snippet } from 'svelte';

  // A titled run of rows inside one Panel, for a section that holds two
  // subjects (the Jellyfin and OpenSubsonic servers, a service's options and
  // its connection).
  let { title, children }: { title: string; children: Snippet } = $props();
</script>

<div class="row-group">
  <h3>{title}</h3>
  {@render children()}
</div>

<style>
  .row-group {
    border-top: 1px solid var(--p11-border-subtle);
  }

  .row-group:first-child {
    border-top: none;
  }

  h3 {
    margin: 0;
    padding: var(--space-sm) var(--space-lg);
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-muted);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
</style>
