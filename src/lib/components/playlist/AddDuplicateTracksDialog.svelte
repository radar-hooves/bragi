<script lang="ts">
  import { getAddDuplicateDialogState, resolveAddDuplicateDialog } from '#lib/utils/addToPlaylist.svelte.js';
  import * as Dialog from '@poodle64/ui/dialog';

  const state = $derived(getAddDuplicateDialogState());

  function handleOpenChange(open: boolean): void {
    if (!open) resolveAddDuplicateDialog('cancel');
  }
</script>

<Dialog.Root open={state.open} onOpenChange={handleOpenChange}>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Already in this playlist</Dialog.Title>
      <Dialog.Description>
        {state.duplicateCount === 1 ? 'One song is' : `${state.duplicateCount} songs are`} already in “{state.playlistName}”.
      </Dialog.Description>
    </Dialog.Header>
    <div class="actions">
      <button class="cancel-btn" onclick={() => resolveAddDuplicateDialog('cancel')}>Cancel</button>
      <button class="skip-btn" onclick={() => resolveAddDuplicateDialog('skip')}>Skip</button>
      <button class="add-btn" onclick={() => resolveAddDuplicateDialog('add')}>Add Anyway</button>
    </div>
  </Dialog.Content>
</Dialog.Root>

<style>
  .actions {
    display: flex;
    gap: var(--space-sm);
    justify-content: flex-end;
    margin-top: var(--space-lg);
  }

  .cancel-btn,
  .skip-btn,
  .add-btn {
    padding: var(--space-xs) var(--space-md);
    border-radius: var(--p11-radius-md);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .cancel-btn,
  .skip-btn {
    background: transparent;
    border: 1px solid var(--p11-border);
    color: var(--p11-text-primary);
  }

  .cancel-btn:hover,
  .skip-btn:hover {
    background-color: var(--p11-bg-elevated);
  }

  .add-btn {
    background-color: var(--p11-accent);
    border: none;
    color: rgb(var(--p11-overlay-rgb));
  }

  .add-btn:hover {
    background-color: var(--p11-accent-hover);
  }
</style>
