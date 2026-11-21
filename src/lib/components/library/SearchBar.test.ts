// Debounce, immediate-on-Return, and clear/Escape — the one search box every
// entry point (⌘F, the toolbar icon) shares.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import { clearSearch } from '#lib/stores/ui.svelte.js';
import SearchBar from './SearchBar.svelte';

async function expandAndGetInput() {
  await fireEvent.click(screen.getByRole('button', { name: 'Search library' }));
  return screen.getByPlaceholderText('Search library...') as HTMLInputElement;
}

describe('SearchBar', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    clearSearch();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('debounces onSearch while typing, firing once after the delay', async () => {
    const onSearch = vi.fn();
    render(SearchBar, { props: { onSearch, debounceMs: 300 } });
    const input = await expandAndGetInput();

    await fireEvent.input(input, { target: { value: 'roc' } });
    vi.advanceTimersByTime(299);
    expect(onSearch).not.toHaveBeenCalled();

    await fireEvent.input(input, { target: { value: 'rock' } });
    vi.advanceTimersByTime(299);
    expect(onSearch).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(onSearch).toHaveBeenCalledTimes(1);
    expect(onSearch).toHaveBeenCalledWith('rock');
  });

  it('searches immediately on Return, cancelling the pending debounce, and activates the top result', async () => {
    const onSearch = vi.fn();
    render(SearchBar, { props: { onSearch, debounceMs: 300 } });
    const input = await expandAndGetInput();

    await fireEvent.input(input, { target: { value: 'rock' } });
    await fireEvent.submit(input.closest('form')!);

    expect(onSearch).toHaveBeenCalledTimes(1);
    expect(onSearch).toHaveBeenCalledWith('rock', true);

    // The debounced call from the input above must not also fire.
    vi.advanceTimersByTime(300);
    expect(onSearch).toHaveBeenCalledTimes(1);
  });

  it('clears the query and reports an empty search on the clear button', async () => {
    const onSearch = vi.fn();
    render(SearchBar, { props: { onSearch, debounceMs: 300 } });
    const input = await expandAndGetInput();

    await fireEvent.input(input, { target: { value: 'rock' } });
    vi.advanceTimersByTime(300);
    onSearch.mockClear();

    await fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));
    expect(onSearch).toHaveBeenCalledWith('');
    expect(input.value).toBe('');
  });

  it('clears the query on Escape when there is one, closing on a second Escape', async () => {
    const onSearch = vi.fn();
    render(SearchBar, { props: { onSearch, debounceMs: 300 } });
    const input = await expandAndGetInput();

    await fireEvent.input(input, { target: { value: 'rock' } });
    vi.advanceTimersByTime(300);
    onSearch.mockClear();

    await fireEvent.keyDown(input, { key: 'Escape' });
    expect(onSearch).toHaveBeenCalledWith('');
    expect(screen.getByPlaceholderText('Search library...')).toBeInTheDocument();

    await fireEvent.keyDown(input, { key: 'Escape' });
    expect(screen.queryByPlaceholderText('Search library...')).not.toBeInTheDocument();
  });
});
