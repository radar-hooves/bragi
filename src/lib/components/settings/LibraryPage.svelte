<script lang="ts">
  import { onMount } from 'svelte';
  import * as Select from '@poodle64/ui/select';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import JellyfinConnection from './JellyfinConnection.svelte';
  import SubsonicConnection from './SubsonicConnection.svelte';
  import LibrarySyncSection from './LibrarySyncSection.svelte';
  import {
    setActiveBackend,
    getActiveBackend,
    getIsJellyfinConfigured,
    getIsSubsonicConfigured,
    refreshJellyfinStatus,
  } from '#lib/stores/jellyfin.svelte.js';

  type ServerType = 'jellyfin' | 'opensubsonic';
  const SERVER_TYPES: { id: ServerType; label: string }[] = [
    { id: 'jellyfin', label: 'Jellyfin' },
    { id: 'opensubsonic', label: 'OpenSubsonic (Navidrome, Gonic, Airsonic)' },
  ];

  // Both backends can be connected at once. When both are, an active
  // toggle switches which library is shown & played; the switch is instant and
  // re-sync-free (both libraries are cached). When only one is connected it is
  // active automatically, so the toggle is hidden.
  const bothConnected = $derived(getIsJellyfinConfigured() && getIsSubsonicConfigured());
  const activeValue = $derived<ServerType>(getActiveBackend() ?? 'jellyfin');
  const activeLabel = $derived(
    SERVER_TYPES.find((s) => s.id === activeValue)?.label ?? 'Jellyfin'
  );

  // Refresh per-backend status when opening Settings so the cards/toggle are
  // current (the layout already inits at boot; this is a cheap resync).
  onMount(() => {
    void refreshJellyfinStatus();
  });

  async function handleActiveBackendChange(value: string | undefined): Promise<void> {
    if (value !== 'jellyfin' && value !== 'opensubsonic') return;
    if (value === getActiveBackend()) return; // no-op
    const configured = value === 'jellyfin' ? getIsJellyfinConfigured() : getIsSubsonicConfigured();
    if (!configured) return; // can't activate a backend with no stored creds
    try {
      await setActiveBackend(value);
    } catch (e) {
      console.error('Failed to switch backend:', e);
    }
  }
</script>

<Panel
  title="Servers"
  description="Connect Jellyfin, an OpenSubsonic server (Navidrome, Gonic, Airsonic), or both."
  pad={false}
>
  {#if bothConnected}
    <SettingsRow
      label="Active server"
      description="Choose which library Bragi shows and plays. Switching is instant and does not re-sync."
      controlId="active-backend"
    >
      <Select.Root type="single" value={activeValue} onValueChange={handleActiveBackendChange}>
        <Select.Trigger id="active-backend">{activeLabel}</Select.Trigger>
        <Select.Content>
          {#each SERVER_TYPES as s (s.id)}
            <Select.Item value={s.id}>{s.label}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    </SettingsRow>
  {/if}

  <JellyfinConnection />
  <SubsonicConnection />
</Panel>

<LibrarySyncSection />
