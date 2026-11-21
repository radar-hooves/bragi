<script lang="ts">
  // A slider with its value read-out, for a SettingsRow control. The package
  // ships no slider; this stays local until it does.
  interface Props {
    id?: string;
    min: number;
    max: number;
    step: number;
    value: number;
    display: string;
    onchange: (value: number) => void;
  }

  let { id, min, max, step, value, display, onchange }: Props = $props();
</script>

<div class="range-control">
  <input
    {id}
    type="range"
    {min}
    {max}
    {step}
    {value}
    oninput={(e) => onchange(parseFloat(e.currentTarget.value))}
    class="range"
  />
  <span class="range-value">{display}</span>
</div>

<style>
  .range-control {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    width: 14rem;
  }

  .range {
    flex: 1;
    min-width: 0;
    height: 4px;
    appearance: none;
    background: var(--p11-border);
    border-radius: var(--radius-full);
    cursor: pointer;
  }

  .range::-webkit-slider-thumb {
    appearance: none;
    width: 16px;
    height: 16px;
    background: var(--p11-accent);
    border-radius: var(--radius-full);
    cursor: pointer;
  }

  .range::-moz-range-thumb {
    width: 16px;
    height: 16px;
    background: var(--p11-accent);
    border: none;
    border-radius: var(--radius-full);
    cursor: pointer;
  }

  .range-value {
    min-width: 4ch;
    text-align: right;
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
    color: var(--p11-text-secondary);
  }
</style>
