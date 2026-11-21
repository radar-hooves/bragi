import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import ColumnBrowserColumn from './ColumnBrowserColumn.svelte';
import { clearScrollMemory } from '#lib/utils/scrollMemory.svelte.js';

const VIEWPORT_WIDTH = 250;
const VIEWPORT_HEIGHT = 400;
let widthDescriptor: PropertyDescriptor | undefined;
let heightDescriptor: PropertyDescriptor | undefined;

function makeItems(n: number) {
  return Array.from({ length: n }, (_, i) => ({
    id: `item-${i}`,
    label: `Item ${i}`,
    selected: false,
  }));
}

function baseProps(count: number) {
  return {
    title: 'ARTISTS',
    items: makeItems(count),
    allCount: count,
    allSelected: true,
    onItemClick: vi.fn(),
    onAllClick: vi.fn(),
  };
}

describe('ColumnBrowserColumn', () => {
  beforeEach(() => {
    widthDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth');
    heightDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight');
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get: () => VIEWPORT_WIDTH,
    });
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get: () => VIEWPORT_HEIGHT,
    });
  });

  afterEach(() => {
    cleanup();
    clearScrollMemory();
    if (widthDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientWidth', widthDescriptor);
    if (heightDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientHeight', heightDescriptor);
  });

  it('windows a large column: fewer buttons mount than items exist, plus the always-present "All" row', () => {
    const { container } = render(ColumnBrowserColumn, { props: baseProps(2000) });
    const buttons = container.querySelectorAll('.column-item');
    expect(buttons.length).toBeLessThan(2001);
    expect(container.querySelector('.all-item')).toBeTruthy();
  });

  it('End jumps to the last item even though it is far outside the rendered window', async () => {
    const { container } = render(ColumnBrowserColumn, { props: baseProps(2000) });
    const list = container.querySelector<HTMLElement>('.column-list')!;

    await fireEvent.keyDown(list, { key: 'End' });

    // scrollToFocused() can't scrollIntoView an unmounted row, so it must have
    // fallen back to the virtualiser's own scrollToIndex — proven by the
    // viewport actually having moved close to the bottom of the list.
    expect(list.scrollTop).toBeGreaterThan(1900 * 28 * 0.5);
  });

  it('Home returns to the very top ("All" row), scrollTop 0', async () => {
    const { container } = render(ColumnBrowserColumn, { props: baseProps(2000) });
    const list = container.querySelector<HTMLElement>('.column-list')!;

    await fireEvent.keyDown(list, { key: 'End' });
    expect(list.scrollTop).toBeGreaterThan(0);

    await fireEvent.keyDown(list, { key: 'Home' });
    expect(list.scrollTop).toBe(0);
  });

  it('remembers scroll position per column title and resets for a different column', async () => {
    const first = render(ColumnBrowserColumn, { props: baseProps(2000) });
    const list1 = first.container.querySelector<HTMLElement>('.column-list')!;
    list1.scrollTop = 500;
    first.unmount();

    const second = render(ColumnBrowserColumn, { props: baseProps(2000) });
    // The restore is queued a microtask out, so it can never re-enter
    // Svelte's own scheduler mid-effect — give it one turn.
    await Promise.resolve();
    const list2 = second.container.querySelector<HTMLElement>('.column-list')!;
    expect(list2.scrollTop).toBe(500);
    second.unmount();

    const third = render(ColumnBrowserColumn, {
      props: { ...baseProps(2000), title: 'ALBUMS' },
    });
    await Promise.resolve();
    const list3 = third.container.querySelector<HTMLElement>('.column-list')!;
    expect(list3.scrollTop).toBe(0); // a different column starts fresh
  });
});
