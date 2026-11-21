import { clearSearch, getHasSearchQuery, setCurrentSection, setColumnBrowserVisible, type LibrarySection } from './ui.svelte';
import {
  snapshotColumnBrowser,
  restoreColumnBrowser,
  type ColumnBrowserSnapshot,
} from './columnBrowser.svelte';

/** Snapshot of navigation state at a point in time. */
interface NavigationEntry {
  section: LibrarySection;
  /** Jellyfin ID of selected artist (artists section). */
  selectedArtistId: string | null;
  /** Jellyfin ID of selected album filter (tracks section via album click). */
  selectedAlbumId: string | null;
  /** Whether the column browser was visible. */
  columnBrowserVisible: boolean;
  /**
   * The column browser selection in force when this entry was pushed.
   *
   * Captured from the store rather than passed by the caller, so an entry can
   * never disagree with what the browser was actually showing — including the
   * genre drill-down and multi-select, which the old selectedAlbumId-only
   * restore silently dropped.
   */
  columnBrowser: ColumnBrowserSnapshot;
}

interface NavigationState {
  history: NavigationEntry[];
  cursor: number;
}

const MAX_HISTORY = 50;

const emptySelection = (): ColumnBrowserSnapshot => ({ genres: [], artists: [], albums: [] });

const initialState: NavigationState = {
  history: [
    {
      section: 'albums',
      selectedArtistId: null,
      selectedAlbumId: null,
      columnBrowserVisible: false,
      columnBrowser: emptySelection(),
    },
  ],
  cursor: 0,
};

let navigationState = $state<NavigationState>({ ...initialState });

/** Whether back navigation is available. */
export function getCanGoBack(): boolean {
  return getHasSearchQuery() || navigationState.cursor > 0;
}

/** Whether forward navigation is available. */
export function getCanGoForward(): boolean {
  return navigationState.cursor < navigationState.history.length - 1;
}

/** Flag to suppress history recording when restoring a navigation entry. */
let isRestoring = false;

/**
 * Apply a navigation entry to the UI state without recording history.
 */
function applyEntry(entry: NavigationEntry): void {
  isRestoring = true;
  try {
    setCurrentSection(entry.section);
    restoreColumnBrowser(entry.columnBrowser);
    setColumnBrowserVisible(entry.columnBrowserVisible);

    if (entry.section === 'artists') {
      window.dispatchEvent(
        new CustomEvent('bragi-nav-restore-artist', { detail: entry.selectedArtistId })
      );
    }
  } finally {
    isRestoring = false;
  }
}

/**
 * Navigate to a new view, pushing it onto the history stack.
 */
export function navigateTo(
  entry: Omit<NavigationEntry, 'columnBrowserVisible' | 'columnBrowser'> & {
    columnBrowserVisible?: boolean;
  }
): void {
  if (isRestoring) return;

  const fullEntry: NavigationEntry = {
    columnBrowserVisible: false,
    ...entry,
    // Callers sync the column browser before navigating, so the store is the
    // truth about what this entry shows.
    columnBrowser: snapshotColumnBrowser(),
  };

  const current = navigationState.history[navigationState.cursor];
  if (
    current &&
    current.section === fullEntry.section &&
    current.selectedArtistId === fullEntry.selectedArtistId &&
    current.selectedAlbumId === fullEntry.selectedAlbumId
  ) {
    return;
  }

  const history = navigationState.history.slice(0, navigationState.cursor + 1);
  history.push(fullEntry);

  if (history.length > MAX_HISTORY) {
    history.shift();
  }
  navigationState.history = history;
  navigationState.cursor = history.length - 1;
}

/** Navigate back in history. */
export function navigateBack(): void {
  // Search temporarily replaces the current view; dismiss it before moving
  // history so playing a result never loses the artist we came from.
  if (getHasSearchQuery()) {
    clearSearch();
    return;
  }
  if (navigationState.cursor <= 0) return;
  const newCursor = navigationState.cursor - 1;
  const entry = navigationState.history[newCursor];
  navigationState.cursor = newCursor;
  if (entry) applyEntry(entry);
}

/** Navigate forward in history. */
export function navigateForward(): void {
  if (navigationState.cursor >= navigationState.history.length - 1) return;
  const newCursor = navigationState.cursor + 1;
  const entry = navigationState.history[newCursor];
  navigationState.cursor = newCursor;
  if (entry) applyEntry(entry);
}

/**
 * Initialise navigation with the current UI state.
 * Call once at app startup after UI state is loaded.
 */
export function initNavigation(section: LibrarySection): void {
  navigationState = {
    history: [
      {
        section,
        selectedArtistId: null,
        selectedAlbumId: null,
        columnBrowserVisible: false,
        columnBrowser: emptySelection(),
      },
    ],
    cursor: 0,
  };
}
