<script lang="ts">
  interface Props {
    size?: number;
    colour?: string;
  }

  let { size = 24, colour = 'var(--p11-accent)' }: Props = $props();
</script>

<svg
  class="spinner"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  aria-label="Loading"
>
  <circle class="track" cx="12" cy="12" r="10" stroke="var(--p11-bg-elevated)" stroke-width="3" />
  <circle
    class="indicator"
    cx="12"
    cy="12"
    r="10"
    stroke={colour}
    stroke-width="3"
    stroke-linecap="round"
    stroke-dasharray="31.4 31.4"
  />
</svg>

<style>
  .spinner {
    animation: rotate 1s linear infinite;
  }

  .indicator {
    animation: dash 1.5s ease-in-out infinite;
    transform-origin: center;
  }

  @keyframes rotate {
    100% {
      transform: rotate(360deg);
    }
  }

  @keyframes dash {
    0% {
      stroke-dasharray: 1 62.8;
      stroke-dashoffset: 0;
    }
    50% {
      stroke-dasharray: 31.4 31.4;
      stroke-dashoffset: -15.7;
    }
    100% {
      stroke-dasharray: 1 62.8;
      stroke-dashoffset: -62.8;
    }
  }
</style>
