<script lang="ts">
  import { toast } from 'svelte-sonner';
  import { Switch } from '@poodle64/ui/switch';
  import { Button } from '@poodle64/ui/button';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import * as Select from '@poodle64/ui/select';
  import {
    getUiState,
    setStatusBarVisible,
    setTitleBarVisible,
    setThemeMode,
    setAccentColour,
    setUiZoomLevel,
    ACCENT_COLOURS,
    UI_ZOOM_LEVELS,
    type AccentColour,
    type ThemeMode,
  } from '#lib/stores/ui.svelte.js';
  import {
    getPreferences,
    setSongNotificationsEnabled,
    setStartupLaunchOnLogin,
    setStartMinimised,
    setCloseToTray,
    setMinimiseToTray,
  } from '#lib/stores/preferences.svelte.js';
  import { sendTestNotification } from '#lib/services/tauri.js';

  // Platform detection for Linux-only settings
  const isLinux = typeof navigator !== 'undefined' &&
    navigator.userAgent.toLowerCase().includes('linux');
  // On macOS the red close button always keeps the app in the tray (so the
  // tray's Show/Quit never dies); the "Close to tray" toggle has no effect
  // there, so it shows the fixed behaviour instead. Minimise-to-tray still
  // applies on macOS.
  const isMac = typeof navigator !== 'undefined' &&
    navigator.userAgent.toLowerCase().includes('mac');

  async function handleTestNotification(): Promise<void> {
    try {
      await sendTestNotification();
    } catch (e) {
      console.error('Failed to send test notification:', e);
    }
  }

  const themeModeOptions: { value: ThemeMode; label: string }[] = [
    { value: 'system', label: 'System' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];

  function handleThemeModeChange(value: string | undefined): void {
    if (!value) return;
    setThemeMode(value as ThemeMode);
  }

  const accentColourOptions: { value: AccentColour; label: string }[] = [
    { value: 'blue', label: 'Blue' },
    { value: 'purple', label: 'Purple' },
    { value: 'pink', label: 'Pink' },
    { value: 'red', label: 'Red' },
    { value: 'orange', label: 'Orange' },
    { value: 'yellow', label: 'Yellow' },
    { value: 'green', label: 'Green' },
    { value: 'teal', label: 'Teal' },
  ];

  function handleZoomLevelChange(value: string | undefined): void {
    if (!value) return;
    setUiZoomLevel(parseInt(value, 10));
  }
</script>

<Panel title="Appearance" pad={false}>
  <SettingsRow
    label="Theme"
    description="Follow your system's light or dark appearance, or override it"
    controlId="theme"
  >
    <Select.Root type="single" value={getUiState().themeMode} onValueChange={handleThemeModeChange}>
      <Select.Trigger id="theme">
        {themeModeOptions.find((o) => o.value === getUiState().themeMode)?.label ?? 'System'}
      </Select.Trigger>
      <Select.Content>
        {#each themeModeOptions as option}
          <Select.Item value={option.value}>{option.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  <SettingsRow
    label="Accent colour"
    description="Choose your preferred accent colour for highlights and interactive elements"
  >
    <div class="colour-picker">
      {#each accentColourOptions as option}
        <button
          class="colour-swatch"
          class:selected={getUiState().accentColour === option.value}
          style:--swatch-colour={ACCENT_COLOURS[option.value].primary}
          onclick={() => setAccentColour(option.value)}
          aria-label={option.label}
          title={option.label}
        >
          {#if getUiState().accentColour === option.value}
            <span class="check-mark">✓</span>
          {/if}
        </button>
      {/each}
    </div>
  </SettingsRow>

  <SettingsRow
    label="UI scale"
    description="Adjust the size of the interface for your display — also {isLinux ? 'Ctrl' : '⌘'}+Plus/Minus/0"
    controlId="ui-scale"
  >
    <Select.Root type="single" value={String(getUiState().uiZoomLevel)} onValueChange={handleZoomLevelChange}>
      <Select.Trigger id="ui-scale">{getUiState().uiZoomLevel}%</Select.Trigger>
      <Select.Content>
        {#each UI_ZOOM_LEVELS as level}
          <Select.Item value={String(level)}>{level}%</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  <SettingsRow
    label="Show status bar"
    description="Display item count, duration, and file size at the bottom of library views"
    controlId="status-bar"
  >
    <Switch
      id="status-bar"
      checked={getUiState().statusBarVisible}
      onCheckedChange={(checked) => setStatusBarVisible(checked)}
    />
  </SettingsRow>

  {#if isLinux}
    <SettingsRow
      label="Window decorations"
      description="Show window title bar with minimize, maximize, and close buttons. Turn off for tiling window managers."
      controlId="window-decorations"
    >
      <Switch
        id="window-decorations"
        checked={getUiState().titleBarVisible}
        onCheckedChange={(checked) => setTitleBarVisible(checked)}
      />
    </SettingsRow>
  {/if}
</Panel>

<Panel title="Startup & tray" pad={false}>
  <SettingsRow
    label="Launch on login"
    description="Automatically start Bragi when you log in to your computer"
    controlId="launch-on-login"
  >
    <Switch
      id="launch-on-login"
      checked={getPreferences().startupLaunchOnLogin}
      onCheckedChange={(checked) => {
        setStartupLaunchOnLogin(checked).catch((e) => {
          toast.error(
            checked ? 'Could not enable launch on login' : 'Could not disable launch on login',
            { description: e instanceof Error ? e.message : String(e) }
          );
        });
      }}
    />
  </SettingsRow>

  <SettingsRow
    label="Start minimised to tray"
    description="Launch hidden in the tray instead of opening the window"
    controlId="start-minimised"
  >
    <Switch
      id="start-minimised"
      checked={getPreferences().startMinimised}
      onCheckedChange={(checked) => setStartMinimised(checked)}
    />
  </SettingsRow>

  {#if !isMac}
    <SettingsRow
      label="Close to tray"
      description="Keep Bragi running in the tray when you close the window instead of quitting"
      controlId="close-to-tray"
    >
      <Switch
        id="close-to-tray"
        checked={getPreferences().closeToTray}
        onCheckedChange={(checked) => setCloseToTray(checked)}
      />
    </SettingsRow>
  {:else}
    <SettingsRow
      label="Close to tray"
      description="On macOS, closing the window always keeps Bragi in the menu-bar tray — use ⌘ + Q (or the tray's Quit) to exit."
    >
      <span class="always">Always</span>
    </SettingsRow>
  {/if}

  <SettingsRow
    label="Minimise to tray"
    description="Hide Bragi to the tray when the window is minimised"
    controlId="minimise-to-tray"
  >
    <Switch
      id="minimise-to-tray"
      checked={getPreferences().minimiseToTray}
      onCheckedChange={(checked) => setMinimiseToTray(checked)}
    />
  </SettingsRow>
</Panel>

<Panel title="Notifications" pad={false}>
  <SettingsRow
    label="Song change notifications"
    description="Show a notification when the current track changes while Bragi is in the background"
    controlId="song-notifications"
  >
    <Switch
      id="song-notifications"
      checked={getPreferences().songNotificationsEnabled}
      onCheckedChange={(checked) => setSongNotificationsEnabled(checked)}
    />
  </SettingsRow>

  <SettingsRow
    label="Test notifications"
    description="Send a test notification to verify they are working on your system"
  >
    <Button variant="outline" size="sm" onclick={handleTestNotification}>Test Notification</Button>
  </SettingsRow>
</Panel>

<style>
  .colour-picker {
    display: flex;
    gap: var(--space-xs);
  }

  .colour-swatch {
    width: 28px;
    height: 28px;
    border-radius: var(--radius-full);
    border: 2px solid transparent;
    background-color: var(--swatch-colour);
    cursor: pointer;
    transition:
      transform var(--transition-fast),
      border-color var(--transition-fast);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
  }

  .colour-swatch:hover {
    transform: scale(1.1);
  }

  .colour-swatch.selected {
    border-color: var(--p11-text-primary);
  }

  .check-mark {
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-bold);
    text-shadow: 0 1px 2px rgb(var(--p11-shade-rgb) / 0.3);
  }

  .always {
    color: var(--p11-text-muted);
    font-size: var(--font-size-sm);
  }
</style>
