// Migration proof: this drives real interaction (not a
// render-only check) through the exact @poodle64/ui primitives the migration
// touched most: a compound component (Dialog, context-shared
// Root/Trigger/Content) and the Switch that dozens of Settings rows depend on.
import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import SharedUiSmoke from './shared-ui-smoke.svelte';

describe('@poodle64/ui consumption (migration proof)', () => {
  it('opens a Dialog on trigger click and shows its content', async () => {
    render(SharedUiSmoke);

    expect(screen.queryByText('Smoke test dialog')).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: 'Open dialog' }));

    await waitFor(() => {
      expect(screen.getByText('Smoke test dialog')).toBeInTheDocument();
    });
  });

  it('toggles the migrated Switch on click', async () => {
    render(SharedUiSmoke);

    const toggle = screen.getByRole('switch', { name: 'smoke-test-switch' });
    expect(toggle).toHaveAttribute('aria-checked', 'false');

    await fireEvent.click(toggle);

    await waitFor(() => {
      expect(toggle).toHaveAttribute('aria-checked', 'true');
    });
  });
});
