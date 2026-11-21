<script lang="ts">
  import {
    getScrobbleState,
    connectMusicService,
    disconnectMusicService,
    testMusicServiceConnection,
    getIsServiceConfigured,
    updateMusicServiceForm,
    resetMusicServiceForm,
  } from '#lib/stores/scrobble.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import { Input } from '@poodle64/ui/input';
  import { Button } from '@poodle64/ui/button';
  import SettingsRow from '@poodle64/ui/settings-row';
  import RowGroup from './RowGroup.svelte';
  import Notice from './Notice.svelte';
  import ConnectionStatusRows from './ConnectionStatusRows.svelte';

  let isConnecting = $state(false);
  let isTesting = $state(false);

  async function handleConnect(): Promise<void> {
    const { url, apiToken } = getScrobbleState().form;
    if (!url.trim() || !apiToken.trim()) {
      updateMusicServiceForm({ error: 'Please enter both service URL and API token' });
      return;
    }
    isConnecting = true;
    updateMusicServiceForm({ error: null });
    try {
      await connectMusicService(url.trim(), apiToken.trim());
      resetMusicServiceForm();
    } catch (e) {
      updateMusicServiceForm({ error: e instanceof Error ? e.message : String(e) });
    } finally {
      isConnecting = false;
    }
  }

  async function handleDisconnect(): Promise<void> {
    try {
      await disconnectMusicService();
    } catch (e) {
      updateMusicServiceForm({ error: e instanceof Error ? e.message : String(e) });
    }
  }

  async function handleTestConnection(): Promise<void> {
    isTesting = true;
    updateMusicServiceForm({ testResult: null });
    try {
      const result = await testMusicServiceConnection();
      updateMusicServiceForm({ testResult: result });
    } finally {
      isTesting = false;
    }
  }
</script>

<RowGroup title="Scrobble service">
  {#if getIsServiceConfigured()}
    <ConnectionStatusRows
      url={getScrobbleState().serviceStatus.serviceUrl}
      testing={isTesting}
      testResult={getScrobbleState().form.testResult}
      onTest={handleTestConnection}
      onDisconnect={handleDisconnect}
    />
  {:else}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        handleConnect();
      }}
    >
      <SettingsRow
        label="Service URL"
        description="A ListenBrainz-compatible service: ListenBrainz, Maloja or your own scrobble server. For example https://api.listenbrainz.org"
        controlId="service-url"
      >
        <Input
          id="service-url"
          class="w-72"
          type="url"
          placeholder="https://music.example.com"
          value={getScrobbleState().form.url}
          oninput={(e) => updateMusicServiceForm({ url: e.currentTarget.value })}
          disabled={isConnecting}
        />
      </SettingsRow>

      <SettingsRow
        label="API token"
        description="Your ListenBrainz user token, or your server's API key"
        controlId="api-token"
      >
        <Input
          id="api-token"
          class="w-72"
          type="password"
          placeholder="Your API token"
          value={getScrobbleState().form.apiToken}
          oninput={(e) => updateMusicServiceForm({ apiToken: e.currentTarget.value })}
          disabled={isConnecting}
        />
      </SettingsRow>

      <SettingsRow label="Connect">
        <Button type="submit" disabled={isConnecting}>
          {#if isConnecting}
            <Spinner />
            Connecting...
          {:else}
            Connect
          {/if}
        </Button>
      </SettingsRow>

      {#if getScrobbleState().form.error}
        <Notice variant="destructive">{getScrobbleState().form.error}</Notice>
      {/if}
    </form>
  {/if}
</RowGroup>
