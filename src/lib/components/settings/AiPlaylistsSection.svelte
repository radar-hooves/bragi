<script lang="ts">
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { Switch } from '@poodle64/ui/switch';
  import * as Select from '@poodle64/ui/select';
  import { Input } from '@poodle64/ui/input';
  import { Button } from '@poodle64/ui/button';
  import { Textarea } from '@poodle64/ui/textarea';
  import Panel from '@poodle64/ui/panel';
  import SettingsRow from '@poodle64/ui/settings-row';
  import Notice from './Notice.svelte';
  import { Spinner } from '#lib/components/common/index.js';
  import {
    setLlmApiKey,
    clearLlmApiKey,
    testLlmConnection,
    type TestLlmConnectionResult,
  } from '#lib/services/tauri.js';
  import {
    getPreferences,
    setAiPlaylistEnabled,
    setLlmProvider,
    setLlmBaseUrl,
    setLlmModel,
    setLlmSystemPrompt,
    setLlmMaxTokens,
    setLlmHasApiKey,
  } from '#lib/stores/preferences.svelte.js';

  type ProviderId = 'ollama' | 'openai' | 'anthropic';

  interface ProviderDef {
    id: ProviderId;
    label: string;
    baseUrl: string; // default + placeholder
    showBaseUrl: boolean;
    needsKey: boolean;
    keyLabel: string;
    keyPlaceholder: string;
    hint: string;
  }

  const PROVIDERS: ProviderDef[] = [
    {
      id: 'ollama',
      label: 'Ollama (local)',
      baseUrl: 'http://localhost:11434',
      showBaseUrl: true,
      needsKey: false,
      keyLabel: '',
      keyPlaceholder: '',
      hint: 'Runs models locally — free and private. Install Ollama and pull a model (e.g. `ollama pull llama3.2`).',
    },
    {
      id: 'openai',
      label: 'OpenAI-compatible',
      baseUrl: 'https://api.openai.com',
      showBaseUrl: true,
      needsKey: true,
      keyLabel: 'API Key',
      keyPlaceholder: 'sk-…',
      hint: 'OpenAI, or any OpenAI-compatible endpoint (LM Studio, vLLM, a custom gateway).',
    },
    {
      id: 'anthropic',
      label: 'Anthropic (Claude)',
      baseUrl: 'https://api.anthropic.com',
      showBaseUrl: false,
      needsKey: true,
      keyLabel: 'API Key',
      keyPlaceholder: 'sk-ant-…',
      hint: 'Claude via the Anthropic API.',
    },
  ];

  const FALLBACK = PROVIDERS[0] as ProviderDef;

  // Shown as the placeholder so users see the built-in curation prompt and can
  // override it. Kept in sync with DEFAULT_SYSTEM_PROMPT in the Rust backend.
  const DEFAULT_SYSTEM_PROMPT =
    'You are an expert music curator. From the numbered candidate tracks, choose the ones that best match the user’s request and arrange them into a coherent playlist with good flow. Select only from the candidates — never invent tracks. Prefer 15–30 tracks.';

  const MAX_TOKENS_OPTIONS = [256, 512, 1024, 2048, 4096, 8192];

  // The API key is a credential, entered here but never read back — it keeps
  // its own explicit save/clear action, exactly like the Jellyfin and
  // Telemetry connection panes. Everything else on this page (the enable
  // toggle, provider, endpoint, model, prompt, token budget) applies
  // immediately through the shared preferences store.
  let apiKey = $state('');

  let models = $state<string[]>([]);
  let testing = $state(false);
  let testResult = $state<TestLlmConnectionResult | null>(null);
  let savingKey = $state(false);

  const def = $derived(PROVIDERS.find((p) => p.id === getPreferences().llmProvider) ?? FALLBACK);
  const canSaveKey = $derived(def.needsKey && apiKey.trim().length > 0 && !savingKey);

  onMount(() => {
    const model = getPreferences().llmModel;
    // Show the saved model until a connection test refreshes the list.
    if (model) models = [model];
  });

  function handleProviderChange(value: string): void {
    const provider = value as ProviderId;
    setLlmProvider(provider);
    const providerDef = PROVIDERS.find((p) => p.id === provider);
    setLlmBaseUrl(providerDef?.baseUrl ?? '');
    setLlmModel('');
    apiKey = '';
    models = [];
    testResult = null;
    setLlmHasApiKey(false); // unknown for the newly-selected provider until saved/tested
  }

  async function handleTest(): Promise<void> {
    testing = true;
    testResult = null;
    try {
      const result = await testLlmConnection({
        provider: getPreferences().llmProvider,
        baseUrl: getPreferences().llmBaseUrl.trim() || undefined,
        apiKey: apiKey.trim() || undefined,
      });
      testResult = result;
      if (result.ok) {
        models = result.models;
        // Auto-select the first model if the saved one isn't on offer.
        const first = result.models[0];
        if (first && !result.models.includes(getPreferences().llmModel)) {
          setLlmModel(first);
        }
      }
    } catch (e) {
      testResult = { ok: false, models: [], message: e instanceof Error ? e.message : String(e) };
    } finally {
      testing = false;
    }
  }

  async function handleSaveKey(): Promise<void> {
    savingKey = true;
    try {
      await setLlmApiKey(getPreferences().llmProvider, apiKey.trim());
      apiKey = '';
      setLlmHasApiKey(true);
      toast.success('API key saved');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      savingKey = false;
    }
  }

  async function handleClearKey(): Promise<void> {
    try {
      await clearLlmApiKey(getPreferences().llmProvider);
      setLlmHasApiKey(false);
      apiKey = '';
    } catch (e) {
      console.error('Failed to clear key:', e);
    }
  }
</script>

<Panel
  title="AI Playlists"
  description="Generate a playlist from a natural-language prompt using an LLM of your choice. Only track metadata (title, artist, album, genre, year) is ever sent, never audio. Generated playlists are saved as ordinary Jellyfin playlists."
  pad={false}
>
  <SettingsRow label="Enable AI Playlists" controlId="ai-enabled">
    <Switch
      id="ai-enabled"
      checked={getPreferences().aiPlaylistEnabled}
      onCheckedChange={(checked) => setAiPlaylistEnabled(checked)}
    />
  </SettingsRow>

  <SettingsRow label="Provider" description={def.hint} controlId="ai-provider">
    <Select.Root type="single" value={getPreferences().llmProvider} onValueChange={handleProviderChange}>
      <Select.Trigger id="ai-provider">{def.label}</Select.Trigger>
      <Select.Content>
        {#each PROVIDERS as p}
          <Select.Item value={p.id}>{p.label}</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  {#if def.showBaseUrl}
    <SettingsRow label="Endpoint URL" controlId="ai-base-url">
      <Input
        id="ai-base-url"
        class="w-72"
        type="url"
        placeholder={def.baseUrl}
        value={getPreferences().llmBaseUrl}
        oninput={(e) => {
          setLlmBaseUrl(e.currentTarget.value);
          testResult = null;
        }}
      />
    </SettingsRow>
  {/if}

  {#if def.needsKey}
    <SettingsRow
      label={def.keyLabel}
      description={getPreferences().llmHasApiKey
        ? 'A key is saved. Type a new one and save to replace it, or clear it.'
        : 'Entering a key here does not save it. Click Save key.'}
      controlId="ai-key"
    >
      <Input
        id="ai-key"
        class="w-56"
        type="password"
        placeholder={getPreferences().llmHasApiKey ? '•••••••• (saved)' : def.keyPlaceholder}
        value={apiKey}
        oninput={(e) => {
          apiKey = e.currentTarget.value;
          testResult = null;
        }}
      />
      <Button onclick={handleSaveKey} disabled={!canSaveKey}>
        {#if savingKey}
          <Spinner />
          Saving…
        {:else}
          Save key
        {/if}
      </Button>
      {#if getPreferences().llmHasApiKey}
        <Button variant="outline" onclick={handleClearKey}>Clear</Button>
      {/if}
    </SettingsRow>
  {/if}

  <SettingsRow label="Test connection" description="Check the provider and list the models it offers">
    <Button variant="outline" onclick={handleTest} disabled={testing}>
      {#if testing}
        <Spinner />
        Testing…
      {:else}
        Test Connection
      {/if}
    </Button>
  </SettingsRow>
  {#if testResult}
    <Notice variant={testResult.ok ? 'default' : 'destructive'}>{testResult.message}</Notice>
  {/if}

  <SettingsRow label="Model" controlId="ai-model">
    {#if models.length > 0}
      <Select.Root
        type="single"
        value={getPreferences().llmModel}
        onValueChange={(v) => v && setLlmModel(v)}
      >
        <Select.Trigger id="ai-model">{getPreferences().llmModel || 'Select a model'}</Select.Trigger>
        <Select.Content>
          {#each models as m}
            <Select.Item value={m}>{m}</Select.Item>
          {/each}
        </Select.Content>
      </Select.Root>
    {:else}
      <Input
        id="ai-model"
        class="w-80"
        value={getPreferences().llmModel}
        disabled
        placeholder="Test connection to see available models"
      />
    {/if}
  </SettingsRow>

  <SettingsRow
    label="Response length budget"
    description="The maximum length of the model's curation response. Raise it if long requests get cut off."
    controlId="ai-max-tokens"
  >
    <Select.Root
      type="single"
      value={String(getPreferences().llmMaxTokens)}
      onValueChange={(v) => v && setLlmMaxTokens(parseInt(v, 10))}
    >
      <Select.Trigger id="ai-max-tokens">{getPreferences().llmMaxTokens} tokens</Select.Trigger>
      <Select.Content>
        {#each MAX_TOKENS_OPTIONS as tokens}
          <Select.Item value={String(tokens)}>{tokens} tokens</Select.Item>
        {/each}
      </Select.Content>
    </Select.Root>
  </SettingsRow>

  <SettingsRow
    label="Curation prompt (advanced)"
    description="Instructions given to the model when curating. Leave blank to use the built-in prompt shown below; type your own to override it."
    controlId="ai-system-prompt"
    class="flex-col items-stretch"
  >
    <Textarea
      id="ai-system-prompt"
      class="min-h-28 w-full"
      rows={5}
      placeholder={DEFAULT_SYSTEM_PROMPT}
      value={getPreferences().llmSystemPrompt}
      oninput={(e) => setLlmSystemPrompt(e.currentTarget.value)}
    />
  </SettingsRow>
</Panel>
