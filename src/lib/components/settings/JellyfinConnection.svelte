<script lang="ts">
  import {
    getJellyfinState,
    loginJellyfin,
    disconnectJellyfin,
    testJellyfinConnection,
    getIsJellyfinActive,
    getIsJellyfinConfigured,
    getJellyfinBackend,
    checkQuickConnect,
    startQuickConnect,
    pollQuickConnect,
    completeQuickConnect,
    cancelQuickConnect,
    updateJellyfinForm,
    resetJellyfinForm,
    isCleartextServerUrl,
  } from '#lib/stores/jellyfin.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import { Input } from '@poodle64/ui/input';
  import { Button } from '@poodle64/ui/button';
  import * as Tabs from '@poodle64/ui/tabs';
  import SettingsRow from '@poodle64/ui/settings-row';
  import RowGroup from './RowGroup.svelte';
  import Notice from './Notice.svelte';
  import ConnectionStatusRows from './ConnectionStatusRows.svelte';

  // Non-blocking cleartext warning: the entered URL is http:// to a non-loopback
  // host. Login stays enabled (LAN parity with Subsonic).
  const showCleartextWarning = $derived(isCleartextServerUrl(getJellyfinState().form.url));
  // Persistent "unencrypted" badge for an already-connected cleartext server.
  const connectedInsecure = $derived(
    isCleartextServerUrl(getJellyfinBackend().serverUrl ?? '')
  );

  // UI-only state
  let isConnectingJellyfin = $state(false);
  let isTestingJellyfin = $state(false);
  let isCheckingQuickConnect = $state(false);
  let quickConnectPolling = $state(false);
  let quickConnectPollTimer = $state<ReturnType<typeof setInterval> | null>(null);

  // Clean up polling on unmount
  $effect(() => {
    return () => {
      if (quickConnectPollTimer) {
        clearInterval(quickConnectPollTimer);
        quickConnectPollTimer = null;
      }
    };
  });

  // Restore polling state on mount if Quick Connect is active
  $effect(() => {
    if (getJellyfinState().quickConnectSecret && !quickConnectPolling && !quickConnectPollTimer) {
      startQuickConnectPolling();
    }
  });

  async function checkQuickConnectAvailability(): Promise<void> {
    const url = getJellyfinState().form.url.trim();
    if (!url) {
      updateJellyfinForm({ isQuickConnectAvailable: null });
      return;
    }
    isCheckingQuickConnect = true;
    updateJellyfinForm({ error: null });
    try {
      const available = await checkQuickConnect(url);
      updateJellyfinForm({ isQuickConnectAvailable: available });
    } catch (e) {
      updateJellyfinForm({
        isQuickConnectAvailable: false,
        error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      isCheckingQuickConnect = false;
    }
  }

  async function handleLoginJellyfin(): Promise<void> {
    const { url, username, password } = getJellyfinState().form;
    if (!url.trim() || !username.trim() || !password.trim()) {
      updateJellyfinForm({ error: 'Please enter server URL, username, and password' });
      return;
    }
    isConnectingJellyfin = true;
    updateJellyfinForm({ error: null });
    try {
      await loginJellyfin(url.trim(), username.trim(), password.trim());
      resetJellyfinForm();
    } catch (e) {
      updateJellyfinForm({ error: e instanceof Error ? e.message : String(e) });
    } finally {
      isConnectingJellyfin = false;
    }
  }

  async function handleStartQuickConnect(): Promise<void> {
    const url = getJellyfinState().form.url.trim();
    if (!url) {
      updateJellyfinForm({ error: 'Please enter a server URL first' });
      return;
    }
    updateJellyfinForm({ error: null });
    try {
      await startQuickConnect(url);
      startQuickConnectPolling();
    } catch (e) {
      updateJellyfinForm({ error: e instanceof Error ? e.message : String(e) });
    }
  }

  function startQuickConnectPolling(): void {
    quickConnectPolling = true;
    const serverUrl = getJellyfinState().form.url.trim();

    quickConnectPollTimer = setInterval(async () => {
      const secret = getJellyfinState().quickConnectSecret;
      if (!secret) {
        stopQuickConnectPolling();
        return;
      }
      try {
        const authorised = await pollQuickConnect(serverUrl, secret);
        if (authorised) {
          stopQuickConnectPolling();
          await completeQuickConnect(serverUrl, secret);
          resetJellyfinForm();
        }
      } catch (e) {
        updateJellyfinForm({ error: e instanceof Error ? e.message : String(e) });
        stopQuickConnectPolling();
      }
    }, 2000);

    // Timeout after 5 minutes
    setTimeout(
      () => {
        if (quickConnectPolling) {
          stopQuickConnectPolling();
          updateJellyfinForm({ error: 'Quick Connect timed out. Please try again.' });
        }
      },
      5 * 60 * 1000
    );
  }

  function stopQuickConnectPolling(): void {
    quickConnectPolling = false;
    if (quickConnectPollTimer) {
      clearInterval(quickConnectPollTimer);
      quickConnectPollTimer = null;
    }
  }

  function handleCancelQuickConnect(): void {
    stopQuickConnectPolling();
    cancelQuickConnect();
    updateJellyfinForm({ error: null });
  }

  async function handleDisconnectJellyfin(): Promise<void> {
    try {
      await disconnectJellyfin();
    } catch (e) {
      updateJellyfinForm({ error: e instanceof Error ? e.message : String(e) });
    }
  }

  async function handleTestJellyfin(): Promise<void> {
    isTestingJellyfin = true;
    updateJellyfinForm({ testResult: null });
    try {
      const result = await testJellyfinConnection();
      updateJellyfinForm({ testResult: result });
    } finally {
      isTestingJellyfin = false;
    }
  }
</script>

<RowGroup title="Jellyfin">
  {#if getIsJellyfinConfigured()}
    <ConnectionStatusRows
      url={getJellyfinBackend().serverUrl}
      active={getIsJellyfinActive()}
      insecure={connectedInsecure}
      testing={isTestingJellyfin}
      testDisabled={!getIsJellyfinActive()}
      testResult={getJellyfinState().form.testResult}
      onTest={handleTestJellyfin}
      onDisconnect={handleDisconnectJellyfin}
    />
  {:else}
    <SettingsRow
      label="Server URL"
      description={isCheckingQuickConnect
        ? 'Checking server...'
        : getJellyfinState().form.isQuickConnectAvailable === true
          ? 'Quick Connect available'
          : getJellyfinState().form.isQuickConnectAvailable === false &&
              getJellyfinState().form.url.trim()
            ? 'Quick Connect not available — use username & password'
            : 'Your Jellyfin server address'}
      controlId="jellyfin-url"
    >
      <Input
        id="jellyfin-url"
        class="w-72"
        type="text"
        placeholder="jellyfin.example.com"
        value={getJellyfinState().form.url}
        oninput={(e) => updateJellyfinForm({ url: e.currentTarget.value })}
        disabled={isConnectingJellyfin || quickConnectPolling}
        onblur={checkQuickConnectAvailability}
        autocomplete="url"
        autocorrect="off"
        autocapitalize="off"
        spellcheck={false}
      />
    </SettingsRow>
    {#if showCleartextWarning}
      <Notice variant="warning">
        This server uses unencrypted <strong>http://</strong>. Your credentials and all audio would
        travel in the clear over the network. Use <strong>https://</strong> where possible.
      </Notice>
    {/if}

    <div class="auth">
      <Tabs.Root
        value={getJellyfinState().form.authTab}
        onValueChange={(v) =>
          updateJellyfinForm({ authTab: v as 'quick-connect' | 'password', error: null })}
      >
        <Tabs.List class="mx-4">
          <Tabs.Trigger value="quick-connect" disabled={quickConnectPolling}>Quick Connect</Tabs.Trigger>
          <Tabs.Trigger value="password" disabled={quickConnectPolling}>Username & Password</Tabs.Trigger>
        </Tabs.List>

        <Tabs.Content value="quick-connect">
          <div class="quick-connect-panel">
            {#if getJellyfinState().quickConnectCode}
              <p class="qc-instruction">
                Open your Jellyfin dashboard or another client and enter this code:
              </p>
              <div class="qc-code">{getJellyfinState().quickConnectCode}</div>
              {#if quickConnectPolling}
                <div class="qc-status">
                  <Spinner />
                  <span>Waiting for authorisation...</span>
                </div>
              {/if}
              <Button variant="outline" onclick={handleCancelQuickConnect}>Cancel</Button>
            {:else if quickConnectPolling}
              <div class="qc-status">
                <Spinner />
                <span>Initiating Quick Connect...</span>
              </div>
            {:else}
              <p class="qc-description">
                Quick Connect lets you sign in without entering a password. Click the button below,
                then approve the code in the Jellyfin dashboard or on another signed-in device.
              </p>
              <Button
                onclick={handleStartQuickConnect}
                disabled={!getJellyfinState().form.url.trim() ||
                  getJellyfinState().form.isQuickConnectAvailable === false}
              >
                Start Quick Connect
              </Button>
            {/if}
          </div>
        </Tabs.Content>

        <Tabs.Content value="password">
          <form
            onsubmit={(e) => {
              e.preventDefault();
              handleLoginJellyfin();
            }}
          >
            <SettingsRow label="Username" controlId="jellyfin-username">
              <Input
                id="jellyfin-username"
                class="w-72"
                type="text"
                placeholder="Your Jellyfin username"
                value={getJellyfinState().form.username}
                oninput={(e) => updateJellyfinForm({ username: e.currentTarget.value })}
                disabled={isConnectingJellyfin}
                autocomplete="username"
              />
            </SettingsRow>
            <SettingsRow label="Password" controlId="jellyfin-password">
              <Input
                id="jellyfin-password"
                class="w-72"
                type="password"
                placeholder="Your Jellyfin password"
                value={getJellyfinState().form.password}
                oninput={(e) => updateJellyfinForm({ password: e.currentTarget.value })}
                disabled={isConnectingJellyfin}
                autocomplete="current-password"
              />
            </SettingsRow>
            <SettingsRow label="Sign in">
              <Button type="submit" disabled={isConnectingJellyfin}>
                {#if isConnectingJellyfin}
                  <Spinner />
                  Logging in...
                {:else}
                  Log In
                {/if}
              </Button>
            </SettingsRow>
          </form>
        </Tabs.Content>
      </Tabs.Root>
    </div>

    {#if getJellyfinState().form.error}
      <Notice variant="destructive">{getJellyfinState().form.error}</Notice>
    {/if}
  {/if}
</RowGroup>

<style>
  .auth {
    padding: var(--space-md) 0 0;
    border-top: 1px solid var(--p11-border-subtle);
  }

  .quick-connect-panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-lg);
    padding: var(--space-lg);
  }

  .qc-description {
    margin: 0;
    color: var(--p11-text-muted);
    font-size: var(--font-size-sm);
    text-align: center;
    line-height: 1.5;
  }

  .qc-instruction {
    margin: 0;
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    text-align: center;
  }

  .qc-code {
    font-size: var(--font-size-2xl);
    font-weight: var(--font-weight-bold);
    font-family: var(--font-family-mono, monospace);
    letter-spacing: 0.15em;
    color: var(--p11-accent);
    background-color: var(--p11-bg-elevated);
    padding: var(--space-md) var(--space-xl);
    border-radius: var(--p11-radius-md);
    user-select: all;
  }

  .qc-status {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    color: var(--p11-text-muted);
    font-size: var(--font-size-sm);
  }
</style>
