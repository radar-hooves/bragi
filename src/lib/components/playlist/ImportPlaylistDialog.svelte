<script lang="ts">
  import {
    decodePlaylistShareCode,
    parseJellyfinItemLink,
    PlaylistShareCodeError,
    type JellyfinItemLink,
  } from '#lib/utils/playlistShare.js';
  import {
    importPlaylist,
    importPlaylistFromJellyfinLink,
    getImportedPlaylistState,
  } from '#lib/stores/importedPlaylists.svelte.js';
  import { setCurrentSection } from '#lib/stores/ui.svelte.js';
  import type { ImportedPlaylistDetail } from '#lib/types/models.js';
  import { Spinner } from '#lib/components/common/index.js';
  import { Input } from '@poodle64/ui/input';
  import * as Dialog from '@poodle64/ui/dialog';

  interface Props {
    /** Called when dialog is closed (cancelled or successful). */
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let dialogOpen = $state(true);
  let code = $state('');
  let isImporting = $state(false);
  let error = $state<string | null>(null);

  // A pasted Jellyfin item link (rather than a Bragi share code) needs a
  // second step: it carries no track data of its own, so the sender's login
  // is required to fetch it directly from their server.
  let pendingLink = $state<JellyfinItemLink | null>(null);
  let jfUsername = $state('');
  let jfPassword = $state('');

  const step = $derived(pendingLink ? 'jellyfin-credentials' : 'paste');
  const canSubmit = $derived(
    !isImporting &&
      (step === 'paste' ? code.trim().length > 0 : jfUsername.trim().length > 0 && jfPassword.length > 0)
  );

  $effect(() => {
    if (!dialogOpen) onClose();
  });

  function finishImport(imported: ImportedPlaylistDetail | null): void {
    if (imported) {
      setCurrentSection('sharedPlaylists');
      isImporting = false;
      dialogOpen = false;
    } else {
      error = getImportedPlaylistState().error ?? 'Failed to import playlist. Please try again.';
      isImporting = false;
    }
  }

  async function handleSubmit(): Promise<void> {
    if (!canSubmit) return;
    error = null;

    if (step === 'jellyfin-credentials') {
      if (!pendingLink) return;
      isImporting = true;
      const imported = await importPlaylistFromJellyfinLink(
        pendingLink.serverUrl,
        jfUsername,
        jfPassword,
        pendingLink.itemId
      );
      finishImport(imported);
      return;
    }

    const trimmed = code.trim();

    // A plain Jellyfin "copy link" carries no track data — it's a deep link
    // that only resolves for someone already logged into THAT server. Detect
    // it and switch to asking for the login the sender gave us, rather than
    // failing with a generic "invalid code" error.
    if (/^https?:\/\//i.test(trimmed)) {
      const parsed = parseJellyfinItemLink(trimmed);
      if (!parsed) {
        error = "Couldn't find a playlist ID in that link.";
        return;
      }
      pendingLink = parsed;
      return;
    }

    isImporting = true;
    let payload;
    try {
      payload = decodePlaylistShareCode(trimmed);
    } catch (e) {
      error = e instanceof PlaylistShareCodeError ? e.message : "Couldn't read that playlist code.";
      isImporting = false;
      return;
    }

    const imported = await importPlaylist(payload.name, payload.tracks);
    finishImport(imported);
  }

  function handleBack(): void {
    pendingLink = null;
    jfUsername = '';
    jfPassword = '';
    error = null;
  }
</script>

<Dialog.Root bind:open={dialogOpen}>
  <Dialog.Content class="import-playlist-dialog">
    <Dialog.Header>
      <Dialog.Title>Import Shared Playlist</Dialog.Title>
    </Dialog.Header>

    <div class="dialog-body">
      {#if step === 'paste'}
        <p class="description">
          Paste a playlist code shared from someone else's Bragi, or a plain Jellyfin playlist
          link if they don't use Bragi. Tracks that exist in your library will be playable; the
          rest show as unavailable.
        </p>

        <!-- svelte-ignore a11y_autofocus -->
        <textarea
          class="code-input"
          bind:value={code}
          placeholder="Paste a playlist share code or Jellyfin link here"
          autofocus
          aria-label="Playlist share code or Jellyfin link"
        ></textarea>
      {:else if pendingLink}
        <p class="description">
          That's a Jellyfin link, not a Bragi share code — it has no track data of its own. Ask
          whoever sent it to give you a login on <strong>{pendingLink.serverUrl}</strong> (a guest
          account works fine), then enter it below. Bragi uses it once to fetch this playlist and
          never stores it.
        </p>

        <div class="form-group">
          <label for="jf-link-username">Username</label>
          <Input
            id="jf-link-username"
            type="text"
            placeholder="Username on their server"
            bind:value={jfUsername}
            disabled={isImporting}
            autocomplete="off"
          />
        </div>
        <div class="form-group">
          <label for="jf-link-password">Password</label>
          <Input
            id="jf-link-password"
            type="password"
            placeholder="Password on their server"
            bind:value={jfPassword}
            disabled={isImporting}
            autocomplete="off"
          />
        </div>
      {/if}

      {#if error}
        <div class="error-message">{error}</div>
      {/if}

      <div class="dialog-actions">
        {#if step === 'jellyfin-credentials'}
          <button class="cancel-btn" onclick={handleBack} disabled={isImporting}>Back</button>
        {:else}
          <button class="cancel-btn" onclick={() => (dialogOpen = false)} disabled={isImporting}>
            Cancel
          </button>
        {/if}
        <button class="import-btn" onclick={() => void handleSubmit()} disabled={!canSubmit}>
          {#if isImporting}
            <Spinner size={16} />
            Importing...
          {:else}
            Import Playlist
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

  .code-input {
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

  .code-input:focus {
    outline: none;
    border-color: var(--p11-accent);
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-xs);
  }

  .form-group label {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    color: var(--p11-text-secondary);
  }

  .error-message {
    padding: var(--space-sm) var(--space-md);
    background-color: var(--p11-error-bg);
    border-radius: var(--p11-radius-md);
    color: var(--p11-error);
    font-size: var(--font-size-sm);
  }

  .dialog-actions {
    display: flex;
    gap: var(--space-md);
    justify-content: flex-end;
  }

  .cancel-btn,
  .import-btn {
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

  .import-btn {
    background-color: var(--p11-accent);
    border: none;
    color: rgb(var(--p11-overlay-rgb));
  }

  .import-btn:hover:not(:disabled) {
    background-color: var(--p11-accent-hover);
  }

  .cancel-btn:disabled,
  .import-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
