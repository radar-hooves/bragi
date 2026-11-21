<script lang="ts">
  import { onMount } from 'svelte';
  import { Switch } from '@poodle64/ui/switch';
  import { Button } from '@poodle64/ui/button';
  import { Input } from '@poodle64/ui/input';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import * as Select from '@poodle64/ui/select';
  import {
    getEqPresets,
    getEqStatus,
    setEqEnabled,
    setEqPreset,
    setEqBands,
    saveEqProfile,
    deleteEqProfile,
    resetEq,
  } from '#lib/services/tauri.js';
  import type { EqPresetInfo } from '#lib/types/tauri.js';

  // EQ state
  let eqEnabled = $state(false);
  let currentPreset = $state('flat'); // Use lowercase to match backend format
  let availablePresets = $state<EqPresetInfo[]>([]);
  let bands = $state<number[]>(Array(10).fill(0));
  let isLoading = $state(true);
  let bandCount = $state<10 | 18>(10); // Default to 10-band for simplicity

  // Custom-profile save UI (follow-up)
  let newProfileName = $state('');
  let profileError = $state('');

  // Split the preset list into shipped presets and user-saved profiles so the
  // dropdown can group them and the delete affordance only targets user ones.
  const builtinPresets = $derived(availablePresets.filter((p) => p.builtin));
  const userPresets = $derived(availablePresets.filter((p) => !p.builtin));
  // The active selection, when it is a deletable user profile.
  const selectedUserProfile = $derived(
    availablePresets.find((p) => p.name === currentPreset && !p.builtin),
  );
  // A custom, unsaved curve — the only state from which "Save preset" makes sense.
  const isUnsavedCustom = $derived(currentPreset === 'custom');
  // The existing user profile the typed name would overwrite, if any (matched
  // case-insensitively, the same way the backend upserts). Drives the "this will
  // replace an existing preset" affordance so a save can't silently clobber one.
  const nameCollision = $derived.by(() => {
    const n = newProfileName.trim().toLowerCase();
    if (!n) return undefined;
    return userPresets.find((p) => p.name.toLowerCase() === n);
  });

  // Band frequencies for display
  // 10-band: ISO octave bands (simpler, more common)
  const BAND_FREQUENCIES_10 = [
    31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000,
  ];

  // 18-band: superequalizer (more detailed).
  // Must match ffmpeg's superequalizer band centers and SUPEREQ_FREQS in
  // src-tauri/src/audio/eq.rs band-for-band (the last band is 20000Hz).
  const BAND_FREQUENCIES_18 = [
    65, 92, 131, 185, 262, 370, 523, 740, 1047, 1480, 2093, 2960, 4186, 5920, 8372, 11840, 16744, 20000,
  ];

  // Get current band frequencies based on selection
  function getBandFrequencies() {
    return bandCount === 10 ? BAND_FREQUENCIES_10 : BAND_FREQUENCIES_18;
  }

  const MIN_GAIN = -20;
  const MAX_GAIN = 20;

  // dB reference lines drawn behind every slider (every 5dB, high to low).
  const GRID_DB: number[] = [];
  for (let d = MAX_GAIN; d >= MIN_GAIN; d -= 5) {
    GRID_DB.push(d);
  }

  // Anchor map from a 10-band UI index to its nearest 18-band superequalizer
  // index. Mirrors ANCHORS in src-tauri/src/audio/eq.rs so the 18-band preview
  // matches the applied backend filter band-for-band.
  const BAND_ANCHORS_10_TO_18 = [0, 1, 2, 3, 5, 7, 9, 11, 13, 16];

  // Round half away from zero to match Rust's f64::round, so the interpolated
  // preview never disagrees with the backend by a dB at a half-way point.
  function roundHalfAwayFromZero(value: number): number {
    return Math.sign(value) * Math.round(Math.abs(value));
  }

  // Map 10-band UI values up to 18 superequalizer bands using log-frequency
  // interpolation between anchors (mirror of map_10_to_18_bands in eq.rs).
  function mapBands10To18(bands10: number[]): number[] {
    const bands18 = new Array<number>(18).fill(0);

    // Pin each UI band to its anchor superequalizer band.
    BAND_ANCHORS_10_TO_18.forEach((anchor, k) => {
      bands18[anchor] = bands10[k] ?? 0;
    });

    // Fill every band between two anchors along the log-frequency axis.
    for (let k = 0; k < BAND_ANCHORS_10_TO_18.length - 1; k++) {
      const lo = BAND_ANCHORS_10_TO_18[k]!;
      const hi = BAND_ANCHORS_10_TO_18[k + 1]!;
      const gLo = bands18[lo]!;
      const gHi = bands18[hi]!;
      const fLo = Math.log10(BAND_FREQUENCIES_18[lo]!);
      const fHi = Math.log10(BAND_FREQUENCIES_18[hi]!);
      for (let i = lo + 1; i < hi; i++) {
        const t = (Math.log10(BAND_FREQUENCIES_18[i]!) - fLo) / (fHi - fLo);
        bands18[i] = roundHalfAwayFromZero(gLo + t * (gHi - gLo));
      }
    }

    // Hold the top UI value out to the final band (18b/20000Hz sits above the
    // 16kHz anchor at 17b/16744Hz).
    const lastAnchor = BAND_ANCHORS_10_TO_18[BAND_ANCHORS_10_TO_18.length - 1]!;
    for (let i = lastAnchor + 1; i < 18; i++) {
      bands18[i] = bands10[9] ?? 0;
    }

    return bands18;
  }

  onMount(async () => {
    try {
      const [presets, status] = await Promise.all([getEqPresets(), getEqStatus()]);
      availablePresets = presets;
      eqEnabled = status.enabled;
      currentPreset = status.currentPreset;

      // If we have custom bands from persisted settings, use them
      // Otherwise use the preset's bands
      if (status.bands && status.bands.length > 0 && currentPreset === 'custom') {
        // For custom preset, use the persisted bands (always 18 bands from backend)
        // If we're in 10-band mode, extract the relevant bands
        if (bandCount === 10) {
          bands = extractBands10(status.bands);
        } else {
          bands = [...status.bands];
        }
      } else {
        // For built-in presets, use the preset's bands
        const preset = presets.find((p) => p.name === status.currentPreset) || presets[0];
        if (preset) {
          bands = bandCount === 10 ? [...preset.bands10] : [...preset.bands18];
        }
      }
    } catch (e) {
      console.error('Failed to load EQ status:', e);
    } finally {
      isLoading = false;
    }
  });

  // Extract 10-band values from 18-band array (reverse mapping)
  function extractBands10(bands18: number[]): number[] {
    return [
      bands18[0] || 0,   // 31Hz <- 65Hz
      bands18[1] || 0,   // 63Hz <- 92Hz
      bands18[2] || 0,   // 125Hz <- 131Hz
      bands18[3] || 0,   // 250Hz <- 185Hz
      bands18[5] || 0,   // 500Hz <- 370Hz
      bands18[7] || 0,   // 1kHz <- 740Hz
      bands18[9] || 0,   // 2kHz <- 1480Hz
      bands18[11] || 0,  // 4kHz <- 2960Hz
      bands18[13] || 0,  // 8kHz <- 5920Hz
      bands18[16] || 0,  // 16kHz <- 16744Hz
    ];
  }

  async function handleEqToggle(enabled: boolean): Promise<void> {
    eqEnabled = enabled;
    try {
      await setEqEnabled(enabled);
      // When enabling EQ, reset local state to flat (backend already uses flat by default)
      // Note: We don't call setEqPreset here to avoid a double track reload
      if (enabled) {
        currentPreset = 'flat';
        bands = Array(bandCount).fill(0);
      }
    } catch (e) {
      console.error('Failed to set EQ enabled:', e);
      eqEnabled = !enabled; // Revert on error
    }
  }

  async function handlePresetChange(presetName: string | undefined): Promise<void> {
    if (!presetName) return;
    currentPreset = presetName;

    try {
      await setEqPreset(presetName);
      // Update bands from the preset, using the appropriate band array
      const preset = availablePresets.find((p) => p.name === presetName);
      if (preset) {
        // Use bands10 for 10-band EQ, bands18 for 18-band EQ
        bands = bandCount === 10 ? [...preset.bands10] : [...preset.bands18];
      }
    } catch (e) {
      console.error('Failed to set EQ preset:', e);
    }
  }

  // Clamp, apply, and mark a single band as a custom edit. Shared by the slider
  // and the numeric field so the two can never diverge from the applied filter.
  async function commitBand(index: number, value: number): Promise<void> {
    const clamped = Math.max(MIN_GAIN, Math.min(MAX_GAIN, value));
    bands[index] = clamped; // mutate $state in place
    try {
      await setEqBands(bands);
      currentPreset = 'custom'; // Use lowercase to match backend format
    } catch (e) {
      console.error('Failed to set EQ bands:', e);
    }
  }

  function handleBandChange(index: number, event: Event): void {
    const input = event.target as HTMLInputElement;
    void commitBand(index, parseInt(input.value, 10));
  }

  function handleBandInput(index: number, event: Event): void {
    const input = event.target as HTMLInputElement;
    const parsed = parseInt(input.value, 10);
    const clamped = Math.max(MIN_GAIN, Math.min(MAX_GAIN, Number.isNaN(parsed) ? 0 : parsed));
    // Reflect the clamped/coerced value back so the field can't silently
    // diverge from the applied filter (empty/NaN/out-of-range entries).
    input.value = String(clamped);
    void commitBand(index, clamped);
  }

  async function handleReset(): Promise<void> {
    try {
      await resetEq();
      currentPreset = 'flat'; // Use lowercase to match backend format
      bands = Array(bandCount).fill(0);
    } catch (e) {
      console.error('Failed to reset EQ:', e);
    }
  }

  // Persist the current curve as a named profile; the backend echoes back the
  // refreshed preset list (now including the new profile) and makes it active.
  async function handleSaveProfile(): Promise<void> {
    const name = newProfileName.trim();
    if (!name) return;
    profileError = '';
    try {
      availablePresets = await saveEqProfile(name, bands);
      currentPreset = name;
      newProfileName = '';
    } catch (e) {
      profileError = String(e);
      console.error('Failed to save EQ profile:', e);
    }
  }

  async function handleDeleteProfile(): Promise<void> {
    const profile = selectedUserProfile;
    if (!profile) return;
    profileError = '';
    try {
      availablePresets = await deleteEqProfile(profile.name);
      // Backend falls back to flat when the active profile is removed.
      currentPreset = 'flat';
      bands = Array(bandCount).fill(0);
    } catch (e) {
      profileError = String(e);
      console.error('Failed to delete EQ profile:', e);
    }
  }

  function getFrequencyLabel(freq: number): string {
    if (freq >= 1000) {
      return `${(freq / 1000).toFixed(1)}k`;
    }
    return freq.toString();
  }

  function getBandLabel(index: number): string {
    const freqs = getBandFrequencies();
    return getFrequencyLabel(freqs[index] ?? 0);
  }

  async function handleBandCountChange(count: 10 | 18) {
    if (count === bandCount) return;

    // For a custom curve there is no preset to fall back on ('custom' isn't in
    // availablePresets), so convert the current bands to the new resolution
    // instead of snapping back to flat.
    if (currentPreset === 'custom') {
      bands = count === 18 ? mapBands10To18(bands) : extractBands10(bands);
      bandCount = count;
      try {
        await setEqBands(bands);
      } catch (e) {
        console.error('Failed to update EQ after band count change:', e);
      }
      return;
    }

    bandCount = count;
    // Get the current preset and use its bands for the new count
    const preset = availablePresets.find((p) => p.name === currentPreset) || availablePresets[0];
    if (preset) {
      bands = count === 10 ? [...preset.bands10] : [...preset.bands18];
    } else {
      bands = Array(count).fill(0);
    }
    currentPreset = preset?.name || 'flat';
    try {
      // Apply the preset with new band count
      await setEqPreset(currentPreset);
    } catch (e) {
      console.error('Failed to update EQ after band count change:', e);
    }
  }
</script>

<Panel
  title="Equalizer"
  description="Fine-tune audio frequency response to match your headphones, speakers, or personal preference."
  pad={false}
>
  <SettingsRow
    label="Enable equalizer"
    description="Apply EQ adjustments to audio output"
    controlId="eq-enabled"
  >
    <Switch id="eq-enabled" checked={eqEnabled} onCheckedChange={handleEqToggle} />
  </SettingsRow>

  {#if eqEnabled && !isLoading}
    <SettingsRow
      label="Bands"
      description="Choose between 10-band (simpler) or 18-band (detailed)"
      controlId="eq-bands"
    >
      <Select.Root
        type="single"
        value={String(bandCount)}
        onValueChange={(v) => v && handleBandCountChange(Number(v) as 10 | 18)}
      >
        <Select.Trigger id="eq-bands">{bandCount} bands</Select.Trigger>
        <Select.Content>
          <Select.Item value="10">10 bands</Select.Item>
          <Select.Item value="18">18 bands</Select.Item>
        </Select.Content>
      </Select.Root>
    </SettingsRow>

    <SettingsRow
      label="Preset"
      description="Choose from predefined EQ curves or customize manually"
      controlId="eq-preset"
    >
      <Select.Root type="single" value={currentPreset} onValueChange={handlePresetChange}>
        <Select.Trigger id="eq-preset">
          {isUnsavedCustom
            ? 'Custom (unsaved)'
            : (availablePresets.find((p) => p.name === currentPreset)?.displayName ?? currentPreset)}
        </Select.Trigger>
        <Select.Content>
          <Select.Group>
            <Select.GroupHeading>Presets</Select.GroupHeading>
            {#each builtinPresets as preset}
              <Select.Item value={preset.name}>{preset.displayName}</Select.Item>
            {/each}
          </Select.Group>
          {#if userPresets.length > 0}
            <Select.Group>
              <Select.GroupHeading>My presets</Select.GroupHeading>
              {#each userPresets as preset}
                <Select.Item value={preset.name}>{preset.displayName}</Select.Item>
              {/each}
            </Select.Group>
          {/if}
        </Select.Content>
      </Select.Root>
    </SettingsRow>

    {#if isUnsavedCustom || selectedUserProfile}
      <SettingsRow
        label="Custom profile"
        description={isUnsavedCustom
          ? nameCollision
            ? `A preset named "${nameCollision.displayName}" already exists — saving will replace it.`
            : 'Save the current curve as a reusable preset'
          : 'Manage your saved preset'}
      >
        {#if isUnsavedCustom}
          <Input
            class="w-44"
            placeholder="Name this preset"
            maxlength={40}
            aria-label="Preset name"
            bind:value={newProfileName}
            onkeydown={(e) => {
              if (e.key === 'Enter') handleSaveProfile();
            }}
          />
          <Button
            variant={nameCollision ? 'destructive' : 'outline'}
            size="sm"
            onclick={handleSaveProfile}
            disabled={!newProfileName.trim()}
          >
            {nameCollision ? 'Overwrite' : 'Save preset'}
          </Button>
        {:else if selectedUserProfile}
          <Button variant="destructive" size="sm" onclick={handleDeleteProfile}>
            Delete "{selectedUserProfile.displayName}"
          </Button>
        {/if}
      </SettingsRow>
      {#if profileError}
        <p class="profile-error">{profileError}</p>
      {/if}
    {/if}

    <div class="eq-bands">
      {#each bands as band, index}
        <div class="eq-band">
          <span class="band-label">{getBandLabel(index)}</span>
          <div class="slider-container">
            <div class="eq-gridlines" aria-hidden="true">
              {#each GRID_DB as d}
                <span
                  class={'eq-gridline' + (d === 0 ? ' zero' : '')}
                  style="top: {((MAX_GAIN - d) / (MAX_GAIN - MIN_GAIN)) * 100}%"
                ></span>
              {/each}
            </div>
            <input
              type="range"
              min={MIN_GAIN}
              max={MAX_GAIN}
              step="1"
              value={band}
              aria-label={'Gain for ' + getBandLabel(index) + ' Hz band'}
              oninput={(e) => handleBandChange(index, e)}
              class="eq-slider"
            />
          </div>
          <div class="band-value-row">
            <input
              type="number"
              class={'band-value ' + (band > 0 ? 'positive' : band < 0 ? 'negative' : '')}
              min={MIN_GAIN}
              max={MAX_GAIN}
              step="1"
              value={band}
              aria-label={'Gain for ' + getBandLabel(index) + ' Hz band, dB'}
              onchange={(e) => handleBandInput(index, e)}
            />
            <span class="band-unit">dB</span>
          </div>
        </div>
      {/each}
    </div>

    <SettingsRow label="Reset to flat" description="Reset all bands to neutral (no adjustment)">
      <Button variant="outline" size="sm" onclick={handleReset}>Reset EQ</Button>
    </SettingsRow>
  {/if}
</Panel>

<style>
  .eq-bands {
    display: flex;
    gap: var(--space-sm);
    align-items: flex-end;
    justify-content: space-between;
    padding: var(--space-lg) var(--space-md);
    border-top: 1px solid var(--p11-border-subtle);
    overflow-x: auto;
  }

  .eq-band {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-xs);
    min-width: 50px;
  }

  .band-label {
    font-size: var(--font-size-xs);
    color: var(--p11-text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .slider-container {
    position: relative;
    min-height: 140px;
    width: 30px;
  }

  .eq-slider {
    position: absolute;
    left: 50%;
    top: 50%;
    transform-origin: center center;
    transform: translate(-50%, -50%) rotate(-90deg);
    width: 120px;
    height: 4px;
    appearance: none;
    background: transparent;
    cursor: pointer;
    z-index: 2;
    margin: 0;
  }

  /* Vertical slider track */
  .eq-slider::-webkit-slider-runnable-track {
    width: 120px;
    height: 4px;
    background: var(--p11-bg-elevated);
    border-radius: var(--radius-full);
  }

  .eq-slider::-moz-range-track {
    width: 120px;
    height: 4px;
    background: var(--p11-bg-elevated);
    border-radius: var(--radius-full);
  }

  /* Slider thumb (the accent dot) - positioned by the value */
  .eq-slider::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    background: var(--p11-accent);
    border-radius: 50%;
    cursor: pointer;
    transition: transform var(--transition-fast);
    /* Center the thumb on the track */
    margin-top: -5px; /* (14px - 4px) / 2 */
    box-shadow: 0 0 0 3px var(--p11-bg-surface), 0 1px 3px rgb(var(--p11-shade-rgb) / 0.4);
  }

  .eq-slider::-webkit-slider-thumb:hover {
    transform: scale(1.18);
  }

  .eq-slider::-moz-range-thumb {
    width: 14px;
    height: 14px;
    background: var(--p11-accent);
    border: none;
    border-radius: 50%;
    cursor: pointer;
    transition: transform var(--transition-fast);
    box-shadow: 0 0 0 3px var(--p11-bg-surface), 0 1px 3px rgb(var(--p11-shade-rgb) / 0.4);
  }

  .eq-slider::-moz-range-thumb:hover {
    transform: scale(1.18);
  }

  /* dB reference gridlines (every 5 dB), aligned to the 120px slider travel.
     Height must match the .eq-slider track length so lines land on the true
     thumb positions rather than the container edges. */
  .eq-gridlines {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 120px;
    transform: translateY(-50%);
    pointer-events: none;
    z-index: 0;
  }

  .eq-gridline {
    position: absolute;
    left: 0;
    right: 0;
    border-top: 1px dashed var(--p11-text-muted);
    opacity: 0.22;
  }

  /* 0 dB center line is emphasized (solid, brighter) */
  .eq-gridline.zero {
    border-top-style: solid;
    opacity: 0.4;
  }

  /* Value + unit sit on one line under each slider ("3 dB"). */
  .band-value-row {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 2px;
  }

  .band-unit {
    font-size: var(--font-size-2xs);
    color: var(--p11-text-muted);
    user-select: none;
  }

  /* Numeric dB entry — shares the value typography but drops native chrome */
  .band-value {
    font-size: var(--font-size-xs);
    font-family: var(--font-family);
    font-variant-numeric: tabular-nums;
    color: var(--p11-text-secondary);
    width: 30px;
    text-align: right;
    padding: 2px 0;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    appearance: textfield;
    -moz-appearance: textfield;
  }

  .band-value:hover {
    border-color: var(--p11-border);
  }

  .band-value:focus {
    outline: none;
    border-color: var(--p11-accent);
    background: var(--p11-bg-elevated);
  }

  .band-value::-webkit-outer-spin-button,
  .band-value::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  .band-value.positive {
    color: var(--p11-accent);
    font-weight: var(--font-weight-medium);
  }

  .band-value.negative {
    color: var(--p11-text-muted);
  }










  .profile-error {
    margin: 0;
    padding: 0 var(--space-lg) var(--space-md);
    color: var(--p11-error);
    font-size: var(--font-size-xs);
  }

</style>
