<script lang="ts">
  // Telemetry connection. A Dock-launched app inherits no fleet environment, so
  // this pane is the only door on a machine the fleet does not configure; where
  // the environment sets the exporter it wins and the fields show its values,
  // read-only. The bearer token never reaches Bragi: the helper command prints
  // the headers, and only the command is stored.
  import { onMount } from 'svelte';
  import {
    getTelemetrySettings,
    setTelemetrySettings,
    probeTelemetry,
  } from '#lib/services/tauri.js';
  import { Spinner } from '#lib/components/common/index.js';
  import { Input } from '@poodle64/ui/input';
  import { Button } from '@poodle64/ui/button';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import Notice from './Notice.svelte';

  let endpoint = $state('');
  let headersHelper = $state('');
  let fromEnv = $state(false);
  let error = $state<string | null>(null);
  let status = $state<{ ok: boolean; text: string } | null>(null);
  let isSaving = $state(false);
  let isTesting = $state(false);

  const canTest = $derived(!!endpoint.trim() && !isTesting && !isSaving);

  onMount(async () => {
    try {
      const s = await getTelemetrySettings();
      fromEnv = s.fromEnv;
      // The environment's values where it won, this machine's where it did not.
      endpoint = s.fromEnv ? s.endpoint : s.savedEndpoint;
      headersHelper = s.fromEnv ? s.headersHelper : s.savedHeadersHelper;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  });

  async function handleSave(): Promise<void> {
    isSaving = true;
    error = null;
    status = null;
    try {
      const s = await setTelemetrySettings(endpoint, headersHelper);
      endpoint = s.savedEndpoint;
      headersHelper = s.savedHeadersHelper;
      status = {
        ok: true,
        text: s.savedEndpoint ? 'Saved. Bragi now exports to this collector.' : 'Saved. Nothing leaves this machine.',
      };
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isSaving = false;
    }
  }

  async function handleTest(): Promise<void> {
    isTesting = true;
    status = null;
    try {
      const result = await probeTelemetry(endpoint, headersHelper);
      status = {
        ok: result.ok,
        text: result.ok ? 'Ok — the collector accepted a test record.' : `Failed — ${result.message}.`,
      };
    } catch (e) {
      status = { ok: false, text: e instanceof Error ? e.message : String(e) };
    } finally {
      isTesting = false;
    }
  }
</script>

<Panel
  title="Telemetry"
  description="Send Bragi's security events and API traces to your own OpenTelemetry collector. What leaves this machine is fixed in code: no track title, artist, album, search term, playlist name or file path is in it. Leave the endpoint empty and nothing leaves at all."
  pad={false}
>
  {#if fromEnv}
    <Notice>Set by this machine's environment.</Notice>
  {/if}

  <form
    onsubmit={(e) => {
      e.preventDefault();
      handleSave();
    }}
  >
    <SettingsRow
      label="Endpoint"
      description="The collector's OTLP/HTTP base address. Bragi appends /v1/logs and /v1/traces."
      controlId="telemetry-endpoint"
    >
      <Input
        id="telemetry-endpoint"
        class="w-72"
        type="url"
        placeholder="https://otlp.example.com"
        value={endpoint}
        oninput={(e) => {
          endpoint = e.currentTarget.value;
          status = null;
        }}
        disabled={fromEnv || isSaving}
        autocomplete="url"
        autocorrect="off"
        autocapitalize="off"
        spellcheck={false}
      />
    </SettingsRow>

    <SettingsRow
      label="Authorisation helper"
      description="A command that prints a JSON object of headers. That is how the bearer stays out of the app: Bragi runs the command when it connects and never stores what it prints. Leave it empty for a collector that needs no authorisation."
      controlId="telemetry-helper"
    >
      <Input
        id="telemetry-helper"
        class="w-72"
        type="text"
        placeholder="Command that prints authorisation headers"
        value={headersHelper}
        oninput={(e) => {
          headersHelper = e.currentTarget.value;
          status = null;
        }}
        disabled={fromEnv || isSaving}
        autocorrect="off"
        autocapitalize="off"
        spellcheck={false}
      />
    </SettingsRow>

    <SettingsRow label="Save or test">
      <Button type="submit" disabled={fromEnv || isSaving}>
        {#if isSaving}<Spinner /> Saving...{:else}Save{/if}
      </Button>
      <Button type="button" variant="outline" onclick={handleTest} disabled={!canTest}>
        {#if isTesting}<Spinner /> Testing...{:else}Test{/if}
      </Button>
    </SettingsRow>
  </form>

  {#if status}
    <Notice variant={status.ok ? 'default' : 'destructive'}>{status.text}</Notice>
  {/if}

  {#if error}
    <Notice variant="destructive">{error}</Notice>
  {/if}
</Panel>
