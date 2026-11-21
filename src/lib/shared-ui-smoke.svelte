<script lang="ts">
  import { Switch } from '@poodle64/ui/switch';
  import * as Dialog from '@poodle64/ui/dialog';

  let checked = $state(false);
</script>

<Switch bind:checked aria-label="smoke-test-switch" />

<Dialog.Root>
  <Dialog.Trigger>Open dialog</Dialog.Trigger>
  <Dialog.Content>
    <Dialog.Title>Smoke test dialog</Dialog.Title>
  </Dialog.Content>
</Dialog.Root>
