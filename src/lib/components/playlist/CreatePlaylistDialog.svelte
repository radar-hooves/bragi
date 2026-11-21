<script lang="ts">
  import { createJellyfinPlaylist, selectJellyfinPlaylist } from '#lib/stores/jellyfinPlaylist.svelte.js';
  import { setCurrentSection } from '#lib/stores/ui.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import * as Dialog from '@poodle64/ui/dialog';

  interface Props {
    /** Called when dialog is closed (cancelled or successful). */
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let dialogOpen = $state(true);
  let name = $state('');
  let isCreating = $state(false);
  let error = $state<string | null>(null);

  const canCreate = $derived(name.trim().length > 0 && !isCreating);

  $effect(() => {
    if (!dialogOpen) onClose();
  });

  async function handleCreate(): Promise<void> {
    if (!canCreate) return;

    isCreating = true;
    error = null;

    const trimmed = name.trim();
    const playlist = await createJellyfinPlaylist(trimmed);

    if (playlist) {
      selectJellyfinPlaylist(playlist.jellyfinId);
      setCurrentSection('playlists');
      isCreating = false;
      dialogOpen = false;
    } else {
      // createJellyfinPlaylist stores the error in state; surface it inline
      error = 'Failed to create playlist. Please try again.';
      isCreating = false;
    }
  }

  function handleKeyDown(event: KeyboardEvent): void {
    if (event.key === 'Enter' && canCreate) {
      void handleCreate();
    }
  }
</script>

<Dialog.Root bind:open={dialogOpen}>
  <Dialog.Content class="create-playlist-dialog">
    <Dialog.Header>
      <Dialog.Title>New Playlist</Dialog.Title>
    </Dialog.Header>

    <div class="dialog-body">
      <div class="input-row">
        <!-- svelte-ignore a11y_autofocus -->
        <input id="new-playlist-name" type="text" bind:value={name} placeholder="Playlist name" maxlength="255" autofocus aria-label="Playlist name" onkeydown={handleKeyDown} />
      </div>

      {#if error}
        <div class="error-message">{error}</div>
      {/if}

      <div class="dialog-actions">
        <button class="cancel-btn" onclick={() => (dialogOpen = false)} disabled={isCreating} aria-label="Cancel"> Cancel </button>
        <button class="create-btn" onclick={() => void handleCreate()} disabled={!canCreate} aria-label="Create Playlist">
          {#if isCreating}
            <Spinner size={16} />
            Creating...
          {:else}
            Create Playlist
          {/if}
        </button>
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>

<style>
  .dialog-body {
    padding: 0 var(--space-lg) var(--space-lg);
  }

  .input-row {
    margin-bottom: var(--space-md);
  }

  .input-row input {
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    color: var(--p11-text-primary);
    font-size: var(--font-size-md);
    font-family: var(--font-family);
  }

  .input-row input:focus {
    outline: none;
    border-color: var(--p11-accent);
  }

  .error-message {
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-error-bg);
    border-radius: var(--p11-radius-md);
    color: var(--p11-error);
    font-size: var(--font-size-sm);
    margin-bottom: var(--space-lg);
  }

  .dialog-actions {
    display: flex;
    gap: var(--space-md);
    justify-content: flex-end;
    margin-top: var(--space-lg);
  }

  .cancel-btn,
  .create-btn {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-sm) var(--space-lg);
    border-radius: var(--p11-radius-md);
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .cancel-btn {
    background: transparent;
    border: 1px solid var(--p11-border);
    color: var(--p11-text-primary);
  }

  .cancel-btn:hover:not(:disabled) {
    background-color: var(--p11-bg-elevated);
  }

  .create-btn {
    background-color: var(--p11-accent);
    border: none;
    color: rgb(var(--p11-overlay-rgb));
  }

  .create-btn:hover:not(:disabled) {
    background-color: var(--p11-accent-hover);
  }

  .cancel-btn:disabled,
  .create-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
