<script lang="ts">
  import type { Track } from '#lib/types/models.js';
  import * as DropdownMenu from '@poodle64/ui/dropdown-menu';
  import { revealTrackInLibrary } from '#lib/utils/revealInLibrary.svelte.js';
  import Play from '@lucide/svelte/icons/play';
  import ListPlus from '@lucide/svelte/icons/list-plus';
  import ListEnd from '@lucide/svelte/icons/list-end';
  import ListMusic from '@lucide/svelte/icons/list-music';
  import Library from '@lucide/svelte/icons/library';
  import ListX from '@lucide/svelte/icons/list-x';
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';

  interface Props {
    track: Track;
    showAddToPlaylist: boolean;
    /** Offline with no local copy: disable the playback-shaped actions
     * (Play, Play Next, Add to Queue) rather than let them fail once mpv
     * tries to open a stream URL with no network to reach. */
    playbackDisabled?: boolean;
    /** Bindable so a native right-click on the row it belongs to can open the
     * exact same menu the "..." trigger opens — one definition, two openers. */
    open?: boolean;
    onPlay: () => void;
    /** Selected five, right-click one: Play Next/Add to Queue act on the
     * whole selection, same as Add to Playlist — the caller (TrackList,
     * PlaylistView, ...) widens through its own tracksForAction; a caller
     * with no selection concept (a single-track surface) just queues this
     * one track. */
    onQueueNext: () => void;
    onQueueEnd: () => void;
    onAddToPlaylist: () => void;
    /** Present only inside a playlist's own track list. */
    onRemoveFromPlaylist?: () => void;
  }

  let {
    track,
    showAddToPlaylist,
    playbackDisabled = false,
    open = $bindable(false),
    onPlay,
    onQueueNext,
    onQueueEnd,
    onAddToPlaylist,
    onRemoveFromPlaylist,
  }: Props = $props();
</script>

<DropdownMenu.Root bind:open>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <button {...props} class="track-menu-trigger" aria-label="Track actions for {track.name}">
        <EllipsisVertical size={14} />
      </button>
    {/snippet}
  </DropdownMenu.Trigger>
  <DropdownMenu.Content align="end">
    <DropdownMenu.Item disabled={playbackDisabled} onclick={onPlay}>
      <Play size={14} />
      <span>Play</span>
    </DropdownMenu.Item>
    <DropdownMenu.Item disabled={playbackDisabled} onclick={onQueueNext}>
      <ListEnd size={14} />
      <span>Play Next</span>
    </DropdownMenu.Item>
    <DropdownMenu.Item disabled={playbackDisabled} onclick={onQueueEnd}>
      <ListMusic size={14} />
      <span>Add to Queue</span>
    </DropdownMenu.Item>
    {#if showAddToPlaylist}
      <DropdownMenu.Item onclick={onAddToPlaylist}>
        <ListPlus size={14} />
        <span>Add to Playlist</span>
      </DropdownMenu.Item>
    {/if}
    <DropdownMenu.Item onclick={() => revealTrackInLibrary(track)}>
      <Library size={14} />
      <span>Show in Library</span>
    </DropdownMenu.Item>
    {#if onRemoveFromPlaylist}
      <DropdownMenu.Separator />
      <DropdownMenu.Item onclick={onRemoveFromPlaylist}>
        <ListX size={14} />
        <span>Remove from Playlist</span>
      </DropdownMenu.Item>
    {/if}
  </DropdownMenu.Content>
</DropdownMenu.Root>

<style>
  .track-menu-trigger {
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
    transition: all var(--transition-fast);
  }

  .track-menu-trigger:hover {
    background-color: var(--p11-bg-elevated);
    color: var(--p11-text-primary);
  }
</style>
