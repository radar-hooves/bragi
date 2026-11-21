<script lang="ts">
  import * as Dialog from '@poodle64/ui/dialog';
  import { Spinner } from '#lib/components/common/index.js';
  import { toast } from 'svelte-sonner';
  import { generateAiPlaylist, type AiPlaylistDraft } from '#lib/services/tauri.js';
  import {
    createJellyfinPlaylist,
    addTracksToJellyfinPlaylist,
    deleteJellyfinPlaylist,
    selectJellyfinPlaylist,
    loadJellyfinPlaylists,
    getJellyfinPlaylistState,
  } from '#lib/stores/jellyfinPlaylist.svelte.js';
  import { setCurrentSection } from '#lib/stores/ui.svelte.js';
  import Sparkles from '@lucide/svelte/icons/sparkles';

  interface Props {
    /** Called when the dialog closes (cancelled or saved). */
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let dialogOpen = $state(true);
  let prompt = $state('');
  let generating = $state(false);
  let error = $state<string | null>(null);
  let draft = $state<AiPlaylistDraft | null>(null);
  let name = $state('');
  let saving = $state(false);

  const EXAMPLES = [
    'Upbeat indie rock from the 2000s',
    'Mellow acoustic songs for a rainy afternoon',
    'High-energy workout mix',
    'Late-night jazz',
  ];

  const canGenerate = $derived(prompt.trim().length > 0 && !generating);

  $effect(() => {
    if (!dialogOpen) onClose();
  });

  async function handleGenerate(): Promise<void> {
    if (!canGenerate) return;
    generating = true;
    error = null;
    draft = null;
    try {
      const result = await generateAiPlaylist(prompt.trim());
      draft = result;
      name = result.suggestedName;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      generating = false;
    }
  }

  function backToPrompt(): void {
    draft = null;
    error = null;
  }

  async function handleSave(): Promise<void> {
    if (!draft || !name.trim()) return;
    saving = true;
    try {
      const playlist = await createJellyfinPlaylist(name.trim());
      if (!playlist) {
        toast.error(getJellyfinPlaylistState().error ?? 'Could not create the playlist');
        return;
      }
      // The store swallows errors and returns false — don't report success blindly.
      const added = await addTracksToJellyfinPlaylist(
        playlist.jellyfinId,
        draft.tracks.map((t) => t.jellyfinId)
      );
      if (!added) {
        // Roll back the empty playlist so a retry doesn't pile up duplicates.
        await deleteJellyfinPlaylist(playlist.jellyfinId).catch((e) =>
          console.warn('Could not remove the empty playlist:', e)
        );
        toast.error(getJellyfinPlaylistState().error ?? 'Could not add tracks to the playlist');
        return;
      }
      await loadJellyfinPlaylists();
      selectJellyfinPlaylist(playlist.jellyfinId);
      setCurrentSection('playlists');
      toast.success(`Saved “${name.trim()}” — ${draft.tracks.length} tracks`);
      dialogOpen = false;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      saving = false;
    }
  }
</script>

<Dialog.Root bind:open={dialogOpen}>
  <Dialog.Content class="ai-playlist-dialog">
    <Dialog.Header>
      <Dialog.Title>{draft ? 'Review Playlist' : 'AI Playlist'}</Dialog.Title>
    </Dialog.Header>

    <div class="dialog-body">
      {#if !draft}
        <label class="field-label" for="ai-prompt">Describe the playlist you want</label>
        <textarea
          id="ai-prompt"
          bind:value={prompt}
          rows="3"
          placeholder="e.g. upbeat indie rock from the 2000s"
          disabled={generating}
        ></textarea>
        <div class="examples">
          {#each EXAMPLES as ex}
            <button type="button" class="chip" onclick={() => (prompt = ex)} disabled={generating}>
              {ex}
            </button>
          {/each}
        </div>
        <p class="hint">Only track metadata is sent to your configured provider — never audio.</p>

        {#if error}
          <div class="error-message">{error}</div>
        {/if}

        <div class="dialog-actions">
          <button class="cancel-btn" onclick={() => (dialogOpen = false)} disabled={generating}>
            Cancel
          </button>
          <button class="create-btn" onclick={handleGenerate} disabled={!canGenerate}>
            {#if generating}
              <Spinner size={16} />
              Generating…
            {:else}
              <Sparkles size={16} />
              Generate
            {/if}
          </button>
        </div>
      {:else}
        <input class="name-input" bind:value={name} placeholder="Playlist name" maxlength="255" />
        <div class="draft-meta">
          {draft.tracks.length} tracks
          {#if draft.suggestedCount > draft.matchedCount}
            <span class="muted"
              >· {draft.matchedCount} of {draft.suggestedCount} matched your library</span
            >
          {/if}
        </div>
        <ul class="draft-tracks">
          {#each draft.tracks as t, i}
            <li>
              <span class="t-num">{i + 1}</span>
              <span class="t-name">{t.name}</span>
              <span class="t-artist">{t.artistName}</span>
            </li>
          {/each}
        </ul>

        <div class="dialog-actions">
          <button class="cancel-btn" onclick={backToPrompt} disabled={saving}>Back</button>
          <button class="create-btn" onclick={handleSave} disabled={saving || !name.trim()}>
            {#if saving}
              <Spinner size={16} />
              Saving…
            {:else}
              Save Playlist
            {/if}
          </button>
        </div>
      {/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<style>
  .dialog-body {
    padding: 0 var(--space-lg) var(--space-lg);
  }

  .field-label {
    display: block;
    margin-bottom: var(--space-sm);
    font-size: var(--font-size-sm);
    color: var(--p11-text-secondary);
  }

  textarea {
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    color: var(--p11-text-primary);
    font-size: var(--font-size-md);
    font-family: var(--font-family);
    resize: vertical;
  }

  textarea:focus {
    outline: none;
    border-color: var(--p11-accent);
  }

  .examples {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-sm);
    margin-top: var(--space-sm);
  }

  .chip {
    padding: 4px var(--space-sm);
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-full);
    color: var(--p11-text-secondary);
    font-size: var(--font-size-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .chip:hover:not(:disabled) {
    color: var(--p11-text-primary);
    border-color: var(--p11-accent);
  }

  .hint {
    margin: var(--space-sm) 0 0;
    font-size: var(--font-size-xs);
    color: var(--p11-text-muted);
  }

  .name-input {
    width: 100%;
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-bg-elevated);
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
    color: var(--p11-text-primary);
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-medium);
    font-family: var(--font-family);
  }

  .name-input:focus {
    outline: none;
    border-color: var(--p11-accent);
  }

  .draft-meta {
    margin: var(--space-sm) 0;
    font-size: var(--font-size-sm);
    color: var(--p11-text-secondary);
  }

  .muted {
    color: var(--p11-text-muted);
  }

  .draft-tracks {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 320px;
    overflow-y: auto;
    border: 1px solid var(--p11-border);
    border-radius: var(--p11-radius-md);
  }

  .draft-tracks li {
    display: grid;
    grid-template-columns: 28px 1fr 1fr;
    gap: var(--space-sm);
    align-items: center;
    padding: var(--space-xs) var(--space-md);
    font-size: var(--font-size-sm);
  }

  .draft-tracks li:nth-child(even) {
    background-color: var(--p11-bg-elevated);
  }

  .t-num {
    color: var(--p11-text-muted);
    text-align: right;
  }

  .t-name {
    color: var(--p11-text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .t-artist {
    color: var(--p11-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error-message {
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-error-bg);
    border-radius: var(--p11-radius-md);
    color: var(--p11-error);
    font-size: var(--font-size-sm);
    margin-top: var(--space-md);
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
    font-family: var(--font-family);
    cursor: pointer;
    border: 1px solid var(--p11-border);
  }

  .cancel-btn {
    background: transparent;
    color: var(--p11-text-primary);
  }

  .create-btn {
    background-color: var(--p11-accent);
    border-color: var(--p11-accent);
    color: rgb(var(--p11-overlay-rgb));
  }

  .create-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
