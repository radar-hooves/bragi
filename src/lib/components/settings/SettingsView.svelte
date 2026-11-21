<script lang="ts">
  import SettingsShell from '@poodle64/ui/settings-shell';
  import Settings from '@lucide/svelte/icons/settings';
  import Play from '@lucide/svelte/icons/play';
  import Library from '@lucide/svelte/icons/library';
  import Download from '@lucide/svelte/icons/download';
  import Plug from '@lucide/svelte/icons/plug';
  import Wrench from '@lucide/svelte/icons/wrench';
  import Info from '@lucide/svelte/icons/info';
  import GeneralPage from './GeneralPage.svelte';
  import PlaybackPage from './PlaybackPage.svelte';
  import LibraryPage from './LibraryPage.svelte';
  import DownloadsPage from './DownloadsPage.svelte';
  import ServicesPage from './ServicesPage.svelte';
  import AdvancedPage from './AdvancedPage.svelte';
  import AboutPage from './AboutPage.svelte';
  import { consumeSettingsCategory, type SettingsPane } from '#lib/stores/ui.svelte.js';

  const sections: { items: { id: SettingsPane; label: string; icon: typeof Settings }[] }[] = [
    {
      items: [
        { id: 'general', label: 'General', icon: Settings },
        { id: 'playback', label: 'Playback', icon: Play },
        { id: 'library', label: 'Library', icon: Library },
        { id: 'downloads', label: 'Downloads', icon: Download },
        { id: 'services', label: 'Services', icon: Plug },
        { id: 'advanced', label: 'Advanced', icon: Wrench },
        { id: 'about', label: 'About', icon: Info },
      ],
    },
  ];

  // Honour a pending pane request made before mount (e.g. auth expiry routes
  // straight to Library, where the servers live); default to General.
  let activeCategory = $state<SettingsPane>(consumeSettingsCategory() ?? 'general');

  // Pick up a request that arrives while Settings is already open. Reading the
  // pending value tracks it, so a later requestSettingsCategory() re-runs this;
  // the initial mount already consumed any earlier request, so this no-ops then.
  $effect(() => {
    const requested = consumeSettingsCategory();
    if (requested) activeCategory = requested;
  });
</script>

<SettingsShell
  {sections}
  selected={activeCategory}
  onSelect={(id) => (activeCategory = id as SettingsPane)}
>
  {#if activeCategory === 'general'}
    <GeneralPage />
  {:else if activeCategory === 'playback'}
    <PlaybackPage />
  {:else if activeCategory === 'library'}
    <LibraryPage />
  {:else if activeCategory === 'downloads'}
    <DownloadsPage />
  {:else if activeCategory === 'services'}
    <ServicesPage />
  {:else if activeCategory === 'advanced'}
    <AdvancedPage />
  {:else if activeCategory === 'about'}
    <AboutPage />
  {/if}
</SettingsShell>
