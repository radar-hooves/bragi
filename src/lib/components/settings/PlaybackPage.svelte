<script lang="ts">
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { Switch } from '@poodle64/ui/switch';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import * as Select from '@poodle64/ui/select';
  import RangeControl from './RangeControl.svelte';
  import EqualizerSection from './EqualizerSection.svelte';
  import {
    getPreferences,
    setCrossfadeDuration,
    setGaplessPlayback,
    setReplayGainMode,
    setReplayGainPreamp,
    setFadeSettings,
  } from '#lib/stores/preferences.svelte.js';

  const MIN_CROSSFADE = 0;
  const MAX_CROSSFADE = 12;
  const DEFAULT_CROSSFADE = 3;
  const MIN_FADE_MS = 100;
  const MAX_FADE_MS = 1000;
  const FADE_STEP = 50;
  const MIN_PREAMP = -12;
  const MAX_PREAMP = 12;

  // crossfadeDuration is stored as ms; the UI shows an on/off switch plus a
  // seconds slider that only appears once enabled. Toggling on picks the last
  // duration used (or the 3s default), toggling off stores 0.
  let lastCrossfadeSeconds = $state(DEFAULT_CROSSFADE);
  const crossfadeEnabled = $derived(getPreferences().crossfadeDuration > 0);
  const crossfadeDurationSeconds = $derived(
    crossfadeEnabled ? Math.round(getPreferences().crossfadeDuration / 1000) : lastCrossfadeSeconds
  );

  onMount(() => {
    if (getPreferences().crossfadeDuration > 0) {
      lastCrossfadeSeconds = Math.round(getPreferences().crossfadeDuration / 1000);
    }
  });

  function handleCrossfadeToggle(enabled: boolean): void {
    setCrossfadeDuration(enabled ? lastCrossfadeSeconds * 1000 : 0);
  }

  function handleCrossfadeDurationChange(seconds: number): void {
    lastCrossfadeSeconds = seconds;
    setCrossfadeDuration(seconds * 1000);
  }

  function handleFadeToggle(enabled: boolean): void {
    setFadeSettings(enabled, getPreferences().fadeDurationMs).catch((e) => {
      toast.error('Could not update fade on pause', {
        description: e instanceof Error ? e.message : String(e),
      });
    });
  }

  function handleFadeDurationChange(durationMs: number): void {
    setFadeSettings(getPreferences().fadeEnabled, durationMs).catch((e) => {
      toast.error('Could not update fade duration', {
        description: e instanceof Error ? e.message : String(e),
      });
    });
  }

  function handleReplayGainModeChange(value: string | undefined): void {
    if (!value) return;
    setReplayGainMode(value as 'off' | 'track' | 'album');
  }

  const replayGainModeLabel = $derived(
    getPreferences().replayGainMode === 'off'
      ? 'Off'
      : getPreferences().replayGainMode === 'track'
        ? 'Track'
        : 'Album'
  );
</script>

<Panel
  title="Transitions"
  description="How one track hands over to the next, and how playback starts and stops."
  pad={false}
>
  <SettingsRow
    label="Crossfade"
    description="Fade the end of one track into the beginning of the next"
    controlId="crossfade"
  >
    <Switch id="crossfade" checked={crossfadeEnabled} onCheckedChange={handleCrossfadeToggle} />
  </SettingsRow>

  {#if crossfadeEnabled}
    <SettingsRow label="Crossfade duration" controlId="crossfade-duration">
      <RangeControl
        id="crossfade-duration"
        min={MIN_CROSSFADE}
        max={MAX_CROSSFADE}
        step={1}
        value={crossfadeDurationSeconds}
        display="{crossfadeDurationSeconds}s"
        onchange={handleCrossfadeDurationChange}
      />
    </SettingsRow>
  {/if}

  <SettingsRow
    label="Gapless playback"
    description="Eliminate silence between tracks for seamless listening on live albums and concept albums"
    controlId="gapless"
  >
    <Switch
      id="gapless"
      checked={getPreferences().gaplessPlayback}
      onCheckedChange={(checked) => setGaplessPlayback(checked)}
    />
  </SettingsRow>

  <SettingsRow
    label="Fade on pause"
    description="Ramp the volume down on pause/stop and up on resume instead of cutting abruptly"
    controlId="fade"
  >
    <Switch id="fade" checked={getPreferences().fadeEnabled} onCheckedChange={handleFadeToggle} />
  </SettingsRow>

  {#if getPreferences().fadeEnabled}
    <SettingsRow label="Fade duration" controlId="fade-duration">
      <RangeControl
        id="fade-duration"
        min={MIN_FADE_MS}
        max={MAX_FADE_MS}
        step={FADE_STEP}
        value={getPreferences().fadeDurationMs}
        display="{getPreferences().fadeDurationMs} ms"
        onchange={handleFadeDurationChange}
      />
    </SettingsRow>
  {/if}
</Panel>

<Panel
  title="Sound"
  description="Keep loudness consistent across tracks."
  pad={false}
>
  <SettingsRow
    label="Volume normalisation"
    description={getPreferences().replayGainMode === 'off'
      ? 'Volume normalisation is disabled'
      : getPreferences().replayGainMode === 'track'
        ? 'Each track is normalised individually for consistent loudness'
        : 'Album dynamics are preserved — tracks within an album maintain relative loudness'}
    controlId="normalisation-mode"
  >
    <Select.Root
      type="single"
      value={getPreferences().replayGainMode}
      onValueChange={handleReplayGainModeChange}
    >
      <Select.Trigger id="normalisation-mode">{replayGainModeLabel}</Select.Trigger>
      <Select.Content>
        <Select.Item value="off">Off</Select.Item>
        <Select.Item value="track">Track</Select.Item>
        <Select.Item value="album">Album</Select.Item>
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  {#if getPreferences().replayGainMode !== 'off'}
    <SettingsRow
      label="Preamp adjustment"
      description="Increase or decrease the target volume for normalised tracks"
      controlId="preamp"
    >
      <RangeControl
        id="preamp"
        min={MIN_PREAMP}
        max={MAX_PREAMP}
        step={1}
        value={getPreferences().replayGainPreamp}
        display="{getPreferences().replayGainPreamp > 0 ? '+' : ''}{getPreferences().replayGainPreamp} dB"
        onchange={setReplayGainPreamp}
      />
    </SettingsRow>
  {/if}
</Panel>

<EqualizerSection />
