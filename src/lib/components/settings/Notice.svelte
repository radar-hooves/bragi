<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Alert, AlertDescription } from '@poodle64/ui/alert';

  // A full-width message between rows: a form error or a warning about what
  // the user typed.
  let {
    variant = 'default',
    children,
  }: { variant?: 'default' | 'destructive' | 'warning'; children: Snippet } = $props();
</script>

<div class="notice">
  <Alert {variant}>
    <AlertDescription><span>{@render children()}</span></AlertDescription>
  </Alert>
</div>

<style>
  .notice {
    padding: var(--space-md) var(--space-lg);
    border-top: 1px solid var(--p11-border-subtle);
  }
</style>
