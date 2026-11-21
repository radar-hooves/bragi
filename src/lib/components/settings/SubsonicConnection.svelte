<script lang="ts">
  // OpenSubsonic connection UI. Shares the single media connection with
  // Jellyfin via the jellyfin store: loginSubsonic writes state.media, and the
  // shared status/disconnect/test commands report the active backend.
  import {
    loginSubsonic,
    disconnectSubsonic,
    testJellyfinConnection,
    getSubsonicBackend,
    getIsSubsonicActive,
    isCleartextServerUrl,
  } from '#lib/stores/jellyfin.svelte.js';
  import { Spinner } from '#lib/components/common/index.js';
  import { Input } from '@poodle64/ui/input';
  import { Button } from '@poodle64/ui/button';
  import SettingsRow from '@poodle64/ui/settings-row';
  import RowGroup from './RowGroup.svelte';
  import Notice from './Notice.svelte';
  import ConnectionStatusRows from './ConnectionStatusRows.svelte';

  let url = $state('');
  let username = $state('');
  let password = $state('');
  let error = $state<string | null>(null);
  let testResult = $state<boolean | null>(null);
  let isConnecting = $state(false);
  let isTesting = $state(false);

  // Non-blocking cleartext warning: true when the entered URL is http:// to a
  // non-loopback host. Login stays enabled (LAN parity).
  const showCleartextWarning = $derived(isCleartextServerUrl(url));
  // Persistent "unencrypted" badge for an already-connected cleartext server.
  const connectedInsecure = $derived(
    isCleartextServerUrl(getSubsonicBackend().serverUrl ?? '')
  );

  async function handleLogin(): Promise<void> {
    if (!url.trim() || !username.trim() || !password.trim()) {
      error = 'Please enter server URL, username, and password';
      return;
    }
    isConnecting = true;
    error = null;
    try {
      await loginSubsonic(url.trim(), username.trim(), password.trim());
      url = '';
      username = '';
      password = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isConnecting = false;
    }
  }

  async function handleDisconnect(): Promise<void> {
    try {
      await disconnectSubsonic();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function handleTest(): Promise<void> {
    isTesting = true;
    testResult = null;
    try {
      testResult = await testJellyfinConnection();
    } finally {
      isTesting = false;
    }
  }
</script>

<RowGroup title="OpenSubsonic">
  {#if getSubsonicBackend().connected}
    <ConnectionStatusRows
      url={getSubsonicBackend().serverUrl}
      active={getIsSubsonicActive()}
      insecure={connectedInsecure}
      testing={isTesting}
      testDisabled={!getIsSubsonicActive()}
      {testResult}
      onTest={handleTest}
      onDisconnect={handleDisconnect}
    />
  {:else}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        handleLogin();
      }}
    >
      <SettingsRow
        label="Server URL"
        description="Your OpenSubsonic server address (Navidrome, Gonic, Airsonic)."
        controlId="subsonic-url"
      >
        <Input
          id="subsonic-url"
          class="w-72"
          type="text"
          placeholder="music.example.com"
          value={url}
          oninput={(e) => (url = e.currentTarget.value)}
          disabled={isConnecting}
          autocomplete="url"
          autocorrect="off"
          autocapitalize="off"
          spellcheck={false}
        />
      </SettingsRow>
      {#if showCleartextWarning}
        <Notice variant="warning">
          This server uses unencrypted <strong>http://</strong>. Your username, the salted auth
          token, and all audio would travel in the clear over the network. Use
          <strong>https://</strong> where possible.
        </Notice>
      {/if}
      <SettingsRow label="Username" controlId="subsonic-username">
        <Input
          id="subsonic-username"
          class="w-72"
          type="text"
          placeholder="Your username"
          value={username}
          oninput={(e) => (username = e.currentTarget.value)}
          disabled={isConnecting}
          autocomplete="username"
        />
      </SettingsRow>
      <SettingsRow
        label="Password"
        description="Stored securely in your system keyring. Unlike Jellyfin Quick Connect, OpenSubsonic has no token flow, so Bragi must store your password to sign each request."
        controlId="subsonic-password"
      >
        <Input
          id="subsonic-password"
          class="w-72"
          type="password"
          placeholder="Your password"
          value={password}
          oninput={(e) => (password = e.currentTarget.value)}
          disabled={isConnecting}
          autocomplete="current-password"
        />
      </SettingsRow>
      <SettingsRow label="Sign in">
        <Button type="submit" disabled={isConnecting}>
          {#if isConnecting}<Spinner /> Logging in...{:else}Log In{/if}
        </Button>
      </SettingsRow>
      {#if error}
        <Notice variant="destructive">{error}</Notice>
      {/if}
    </form>
  {/if}
</RowGroup>
