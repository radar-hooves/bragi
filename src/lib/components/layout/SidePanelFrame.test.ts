import { describe, it, expect, vi, afterEach } from 'vitest';
import { createRawSnippet } from 'svelte';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import SidePanelFrame from './SidePanelFrame.svelte';
import ListMusic from '@lucide/svelte/icons/list-music';

function textSnippet(text: string) {
  return createRawSnippet(() => ({ render: () => `<p>${text}</p>` }));
}

describe('SidePanelFrame', () => {
  afterEach(() => {
    cleanup();
  });

  it('calls onClose when the close button is clicked', async () => {
    const onClose = vi.fn();
    render(SidePanelFrame, {
      props: {
        title: 'Up Next',
        icon: ListMusic,
        closeLabel: 'Close Up Next',
        onClose,
        children: textSnippet('body'),
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Close Up Next' }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
