<script lang="ts">
  import { StatusBadge } from '@poodle64/ui/status-badge';
  import { Button } from '@poodle64/ui/button';
  import SettingsRow from '@poodle64/ui/settings-row';
  import { Spinner } from '#lib/components/common/index.js';

  // The connected state of a server or service: who it is, whether it is the
  // live one, and the Test / Disconnect actions.
  interface Props {
    url?: string;
    active?: boolean;
    insecure?: boolean;
    testing: boolean;
    testDisabled?: boolean;
    testResult: boolean | null;
    onTest: () => void;
    onDisconnect: () => void;
  }

  let {
    url,
    active = false,
    insecure = false,
    testing,
    testDisabled = false,
    testResult,
    onTest,
    onDisconnect,
  }: Props = $props();
</script>

<SettingsRow label="Status" description={url}>
  <StatusBadge status="success" label="Connected" />
  {#if active}
    <StatusBadge status="primary" label="Active" />
  {/if}
  {#if insecure}
    <StatusBadge status="warning" label="Unencrypted (http)" />
  {/if}
</SettingsRow>

<SettingsRow label="Connection">
  {#if testResult !== null}
    <StatusBadge
      status={testResult ? 'success' : 'error'}
      label={testResult ? 'Connection successful' : 'Connection failed'}
    />
  {/if}
  <Button variant="outline" size="sm" onclick={onTest} disabled={testing || testDisabled}>
    {#if testing}
      <Spinner />
      Testing...
    {:else}
      Test Connection
    {/if}
  </Button>
  <Button variant="destructive" size="sm" onclick={onDisconnect}>Disconnect</Button>
</SettingsRow>
