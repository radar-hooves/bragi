<script generics="T" lang="ts">
  import * as DropdownMenu from '@poodle64/ui/dropdown-menu';
  import Play from '@lucide/svelte/icons/play';
  import ListPlus from '@lucide/svelte/icons/list-plus';
  import ListEnd from '@lucide/svelte/icons/list-end';
  import ListMusic from '@lucide/svelte/icons/list-music';
  import EllipsisVertical from '@lucide/svelte/icons/ellipsis-vertical';

  interface Props {
    item: T;
    /** e.g. "Album actions for Abbey Road" — the trigger's accessible name. */
    ariaLabel: string;
    /** Bindable so a native right-click on the card can open the same menu. */
    open?: boolean;
    onPlay?: (item: T) => void;
    /** Play Next / Add to Queue share one call, parameterised like the
     * backend's own `addToQueue(tracks, position)`. */
    onQueue?: (item: T, position: 'next' | 'end') => void;
    onAddToPlaylist?: (item: T) => void;
  }

  let { item, ariaLabel, open = $bindable(false), onPlay, onQueue, onAddToPlaylist }: Props = $props();
</script>

<span class="card-menu">
  <DropdownMenu.Root bind:open>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <button
          {...props}
          class="card-menu-trigger"
          aria-label={ariaLabel}
          onclick={(e: MouseEvent) => {
            e.stopPropagation();
            (props.onclick as ((e: MouseEvent) => void) | undefined)?.(e);
          }}
        >
          <EllipsisVertical size={14} />
        </button>
      {/snippet}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content align="end">
      <DropdownMenu.Item onclick={() => onPlay?.(item)}>
        <Play size={14} />
        <span>Play</span>
      </DropdownMenu.Item>
      <DropdownMenu.Item onclick={() => onQueue?.(item, 'next')}>
        <ListEnd size={14} />
        <span>Play Next</span>
      </DropdownMenu.Item>
      <DropdownMenu.Item onclick={() => onQueue?.(item, 'end')}>
        <ListMusic size={14} />
        <span>Add to Queue</span>
      </DropdownMenu.Item>
      {#if onAddToPlaylist}
        <DropdownMenu.Item onclick={() => onAddToPlaylist(item)}>
          <ListPlus size={14} />
          <span>Add to Playlist</span>
        </DropdownMenu.Item>
      {/if}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
</span>

<style>
  /* The host card (`.album-card`/`.artist-card`) also carries `collection-card`
     and `position: relative`; this positions identically for both, a sibling
     of the artwork rather than nested inside it (a circular avatar's own
     `overflow: hidden` would clip a child in its corner). */
  .card-menu {
    position: absolute;
    top: calc(var(--space-sm) + var(--space-sm));
    right: calc(var(--space-sm) + var(--space-sm));
    opacity: 0;
    transition: opacity var(--transition-normal);
  }

  :global(.collection-card:hover) .card-menu,
  :global(.collection-card:focus-within) .card-menu {
    opacity: 1;
  }

  .card-menu-trigger {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    background-color: rgb(var(--p11-overlay-rgb) / 0.85);
    border: none;
    border-radius: var(--radius-full);
    color: rgb(var(--p11-shade-rgb));
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .card-menu-trigger:hover {
    background-color: rgb(var(--p11-overlay-rgb));
  }
</style>
