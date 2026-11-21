import type { Album } from '#lib/types/models.js';
import { getRecentlyAddedAlbums, getSettings } from '#lib/services/tauri.js';

/**
 * Recently Added album cache.
 *
 * RecentlyAddedView lives inside an `{#if}` in LibraryContent, so Svelte
 * destroys and recreates it on every section switch. Holding the albums in
 * component-local `$state` meant `onMount` re-queried the backend each time the
 * user so much as passed through the view. Keeping them here — the same
 * `isLoaded` shape recentlyPlayed uses — makes the second visit free.
 */
interface RecentlyAddedState {
  albums: Album[];
  /** The window the cached albums were fetched for, in days. */
  days: number;
  isLoaded: boolean;
  isLoading: boolean;
  error: string | null;
}

let recentlyAddedState = $state<RecentlyAddedState>({
  albums: [],
  days: 30,
  isLoaded: false,
  isLoading: false,
  error: null,
});

export function getRecentlyAddedState(): RecentlyAddedState {
  return recentlyAddedState;
}

/**
 * Load the recently added albums, reusing the cache when it's warm.
 *
 * Pass `force` after a library sync, or when the user retries a failed load.
 * A changed `recentlyAddedDays` setting also invalidates: the cached list was
 * fetched for a different window and would be wrong for the new one.
 */
export async function loadRecentlyAdded(force = false): Promise<void> {
  if (recentlyAddedState.isLoading) return;

  recentlyAddedState.isLoading = true;
  recentlyAddedState.error = null;
  try {
    const settings = await getSettings();
    const days = settings.recentlyAddedDays;

    if (!force && recentlyAddedState.isLoaded && recentlyAddedState.days === days) {
      return;
    }

    recentlyAddedState.albums = await getRecentlyAddedAlbums(days);
    recentlyAddedState.days = days;
    recentlyAddedState.isLoaded = true;
  } catch (e) {
    console.error('Failed to load recently added albums:', e);
    recentlyAddedState.error = e instanceof Error ? e.message : String(e);
  } finally {
    recentlyAddedState.isLoading = false;
  }
}

/**
 * Drop the cache so the next visit refetches.
 *
 * A sync is the only thing that can change what "recently added" contains, so
 * that's where this belongs.
 */
export function invalidateRecentlyAdded(): void {
  recentlyAddedState.isLoaded = false;
}
