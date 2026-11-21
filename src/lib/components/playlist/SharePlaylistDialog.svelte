<script lang="ts">
  import type { JellyfinPlaylistTrack } from '#lib/types/models.js';
  import { encodePlaylistShareCode } from '#lib/utils/playlistShare.js';
  import * as Dialog from '@poodle64/ui/dialog';
  import Copy from '@lucide/svelte/icons/copy';
  import Check from '@lucide/svelte/icons/check';

  interface Props {
    playlistName: string;
    tracks: JellyfinPlaylistTrack[];
    onClose: () => void;
  }

  let { playlistName, tracks, onClose }: Props = $props();

  let dialogOpen = $state(true);
  let copied = $state(false);

  const shareCode = $derived(encodePlaylistShareCode(playlistName, tracks));
  const resolvableCount = $derived(tracks.filter((t) => t.mbid).length);

  $effect(() => {
    if (!dialogOpen) onClose();
  });

  async function handleCopy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(shareCode);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      console.error('Failed to copy playlist share code:', e);
    }
  }
</script>

<Dialog.Root bind:open={dialogOpen}>
  <Dialog.Content class="share-playlist-dialog">
    <Dialog.Header>
      <Dialog.Title>Share "{playlistName}"</Dialog.Title>
    </Dialog.Header>

    <div class="dialog-body">
      <p class="description">
        Anyone who imports this code into their own Bragi will be able to play whichever of these
        {tracks.length}
        {tracks.length === 1 ? 'track' : 'tracks'} already exist in their library — read-only, and
        never written back to your Jellyfin.
      </p>

      {#if resolvableCount < tracks.length}
        <p class="warning">
          {tracks.length - resolvableCount} of {tracks.length} tracks are missing a MusicBrainz recording
          ID and may not be matched on the other end.
        </p>
      {/if}

      <textarea class="share-code" readonly value={shareCode} aria-label="Playlist share code" onclick={(e) => (e.target as HTMLTextAreaElement).select()}></textarea>

      <div class="dialog-actions">
        <button class="close-btn" onclick={() => (dialogOpen = false)}>Close</button>
        <button class="copy-btn" onclick={() => void handleCopy()}>
          {#if copied}
            <Check size={16} />
            Copied
          {:else}
            <Copy size={16} />
            Copy Link
          {/if}
        </button>
      </div>
    </div>
  </Dialog.Content>
</Dialog.Root>

<style>
  .dialog-body {
    padding: 0 var(--space-lg) var(--space-lg);
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
  }

  .description {
    margin: 0;
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    line-height: 1.5;
  }

  .warning {
    margin: 0;
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-error-bg);
    border-radius: var(--p11-radius-md);
    color: var(--p11-error);
    font-size: var(--font-size-sm);
  }

  .share-code {
    width: 100%;
    height: 120px;
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    color: var(--p11-text-primary);
    font-family: var(--font-family-mono, monospace);
    font-size: var(--font-size-xs);
    line-height: 1.4;
    resize: none;
  }

  .share-code:focus {
    outline: none;
    border-color: var(--p11-accent);
  }

  .dialog-actions {
    display: flex;
    gap: var(--space-md);
    justify-content: flex-end;
  }

  .close-btn,
  .copy-btn {
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

  .close-btn {
    background: transparent;
    border: 1px solid var(--p11-border);
    color: var(--p11-text-primary);
  }

  .close-btn:hover {
    background-color: var(--p11-bg-elevated);
  }

  .copy-btn {
    background-color: var(--p11-accent);
    border: none;
    color: rgb(var(--p11-overlay-rgb));
  }

  .copy-btn:hover {
    background-color: var(--p11-accent-hover);
  }
</style>
