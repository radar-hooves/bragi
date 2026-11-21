<script lang="ts">
  import { onMount } from 'svelte';
  import { Switch } from '@poodle64/ui/switch';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import RangeControl from './RangeControl.svelte';
  import ScrobbleConnection from './ScrobbleConnection.svelte';
  import AiPlaylistsSection from './AiPlaylistsSection.svelte';
  import {
    getPreferences,
    setScrobbleEnabled,
    setScrobbleThreshold,
  } from '#lib/stores/preferences.svelte.js';
  import { getScrobbleState, refreshSyncStatus } from '#lib/stores/scrobble.svelte.js';

  const MIN_SCROBBLE_THRESHOLD = 25;
  const MAX_SCROBBLE_THRESHOLD = 90;

  onMount(() => {
    void refreshSyncStatus();
  });
</script>

<Panel
  title="Scrobbling"
  description="Record your listening history and submit it to a ListenBrainz-compatible service."
  pad={false}
>
  <SettingsRow
    label="Enable scrobbling"
    description="Record your listening history and submit to connected services"
    controlId="scrobble-enabled"
  >
    <Switch
      id="scrobble-enabled"
      checked={getPreferences().scrobbleEnabled}
      onCheckedChange={setScrobbleEnabled}
    />
  </SettingsRow>

  {#if getPreferences().scrobbleEnabled}
    <SettingsRow
      label="Scrobble threshold"
      description="Minimum percentage of track played before scrobbling (or 4 minutes, whichever is less)"
      controlId="scrobble-threshold"
    >
      <RangeControl
        id="scrobble-threshold"
        min={MIN_SCROBBLE_THRESHOLD}
        max={MAX_SCROBBLE_THRESHOLD}
        step={5}
        value={getPreferences().scrobbleThreshold}
        display="{getPreferences().scrobbleThreshold}%"
        onchange={setScrobbleThreshold}
      />
    </SettingsRow>

    <SettingsRow
      label="Offline queue"
      description="Scrobbles are queued locally when offline and synced when back online"
    >
      <span class="value">
        {#if getScrobbleState().syncStatus.pendingCount > 0}
          {getScrobbleState().syncStatus.pendingCount} pending
        {:else}
          No pending scrobbles
        {/if}
      </span>
    </SettingsRow>
  {/if}

  <ScrobbleConnection />
</Panel>

<AiPlaylistsSection />

<style>
  .value {
    color: var(--p11-text-secondary);
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
  }
</style>
