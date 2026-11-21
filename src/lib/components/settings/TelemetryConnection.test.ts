// Asserts the environment-wins branch (`fromEnv`) and the Test button's two
// outcomes.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const getTelemetrySettings = vi.fn();
const setTelemetrySettings = vi.fn();
const probeTelemetry = vi.fn();

vi.mock('#lib/services/tauri.js', () => ({
  getTelemetrySettings: () => getTelemetrySettings(),
  setTelemetrySettings: (e: string, h: string) => setTelemetrySettings(e, h),
  probeTelemetry: (e: string, h: string) => probeTelemetry(e, h),
}));

const { default: TelemetryConnection } = await import('./TelemetryConnection.svelte');

const ENDPOINT = 'https://otlp.example.com';
const HELPER = 'headers-helper otlp';

describe('Telemetry connection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows the environment read-only when the fleet set the exporter', async () => {
    getTelemetrySettings.mockResolvedValue({
      endpoint: ENDPOINT,
      headersHelper: HELPER,
      fromEnv: true,
      savedEndpoint: '',
      savedHeadersHelper: '',
    });

    render(TelemetryConnection);

    await waitFor(() => {
      expect(screen.getByLabelText('Endpoint')).toHaveValue(ENDPOINT);
    });
    expect(screen.getByText("Set by this machine's environment.")).toBeInTheDocument();
    expect(screen.getByLabelText('Endpoint')).toBeDisabled();
    expect(screen.getByLabelText('Authorisation helper')).toHaveValue(HELPER);
    expect(screen.getByLabelText('Authorisation helper')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
  });

  it("edits this machine's saved values when the environment set nothing", async () => {
    getTelemetrySettings.mockResolvedValue({
      endpoint: ENDPOINT,
      headersHelper: HELPER,
      fromEnv: false,
      savedEndpoint: ENDPOINT,
      savedHeadersHelper: HELPER,
    });

    render(TelemetryConnection);

    await waitFor(() => {
      expect(screen.getByLabelText('Endpoint')).toHaveValue(ENDPOINT);
    });
    expect(screen.queryByText("Set by this machine's environment.")).not.toBeInTheDocument();
    expect(screen.getByLabelText('Endpoint')).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();
  });

  it('reports Ok when the collector accepts the probe', async () => {
    getTelemetrySettings.mockResolvedValue({
      endpoint: '',
      headersHelper: '',
      fromEnv: false,
      savedEndpoint: ENDPOINT,
      savedHeadersHelper: HELPER,
    });
    probeTelemetry.mockResolvedValue({ ok: true, message: '' });

    render(TelemetryConnection);
    await waitFor(() => expect(screen.getByLabelText('Endpoint')).toHaveValue(ENDPOINT));

    await fireEvent.click(screen.getByRole('button', { name: 'Test' }));

    await waitFor(() => {
      expect(screen.getByText(/^Ok —/)).toBeInTheDocument();
    });
    expect(probeTelemetry).toHaveBeenCalledWith(ENDPOINT, HELPER);
  });

  it('reports the failure class when the collector refuses', async () => {
    getTelemetrySettings.mockResolvedValue({
      endpoint: '',
      headersHelper: '',
      fromEnv: false,
      savedEndpoint: ENDPOINT,
      savedHeadersHelper: HELPER,
    });
    probeTelemetry.mockResolvedValue({ ok: false, message: 'the collector answered 401' });

    render(TelemetryConnection);
    await waitFor(() => expect(screen.getByLabelText('Endpoint')).toHaveValue(ENDPOINT));

    await fireEvent.click(screen.getByRole('button', { name: 'Test' }));

    await waitFor(() => {
      expect(screen.getByText('Failed — the collector answered 401.')).toBeInTheDocument();
    });
  });
});
