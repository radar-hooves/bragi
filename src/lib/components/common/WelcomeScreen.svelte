<script lang="ts">
  import Music from '@lucide/svelte/icons/music';
  import Server from '@lucide/svelte/icons/server';
  import Settings from '@lucide/svelte/icons/settings';

  interface Props {
    onGoToSettings: () => void;
  }

  let { onGoToSettings }: Props = $props();
</script>

<div class="welcome-screen">
  <div class="welcome-icon">
    <Music size={48} strokeWidth={1.5} />
  </div>
  <h2 class="welcome-title">Welcome to Bragi</h2>
  <p class="welcome-description">
    Connect to your Jellyfin or OpenSubsonic server to start browsing your music library.
  </p>
  <div class="welcome-steps">
    <div class="welcome-step">
      <div class="step-icon"><Server size={20} /></div>
      <div class="step-text">
        <span class="step-label">Music Server</span>
        <span class="step-hint">Log in with your server URL and credentials</span>
      </div>
    </div>
    <div class="welcome-step">
      <div class="step-icon"><Settings size={20} /></div>
      <div class="step-text">
        <span class="step-label">Listening History</span>
        <span class="step-hint"
          >Optionally connect your homelab service for play tracking</span
        >
      </div>
    </div>
  </div>
  <button class="welcome-btn" onclick={onGoToSettings}>
    <Settings size={16} />
    Open Settings
  </button>
</div>

<style>
  .welcome-screen {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--space-3xl) var(--space-xl);
    text-align: center;
    min-height: 400px;
    max-width: 480px;
    margin: 0 auto;
  }

  .welcome-icon {
    width: 72px;
    height: 72px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--p11-accent-subtle), color-mix(in srgb, var(--p11-accent) 6%, transparent));
    border-radius: var(--radius-full);
    color: var(--p11-accent);
    margin-bottom: var(--space-xl);
    box-shadow: 0 0 40px var(--p11-now-playing);
  }

  .welcome-title {
    font-size: var(--font-size-2xl);
    font-weight: var(--font-weight-bold);
    color: var(--p11-text-primary);
    margin: 0 0 var(--space-sm) 0;
  }

  .welcome-description {
    color: var(--p11-text-secondary);
    line-height: var(--line-height-normal);
    margin: 0 0 var(--space-2xl) 0;
  }

  .welcome-steps {
    display: flex;
    flex-direction: column;
    gap: var(--space-md);
    width: 100%;
    margin-bottom: var(--space-2xl);
  }

  .welcome-step {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-md) var(--space-lg);
    background-color: rgb(var(--p11-overlay-rgb) / 0.03);
    border: 1px solid rgb(var(--p11-overlay-rgb) / 0.04);
    border-radius: var(--p11-radius-md);
    text-align: left;
  }

  .step-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    flex-shrink: 0;
    background-color: var(--p11-accent-subtle);
    border-radius: var(--p11-radius-md);
    color: var(--p11-accent);
  }

  .step-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .step-label {
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-medium);
    color: var(--p11-text-primary);
  }

  .step-hint {
    font-size: var(--font-size-sm);
    color: var(--p11-text-muted);
  }

  .welcome-btn {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: 10px var(--space-xl);
    /* Derived from the accent, not a fixed blue: the second stop used to stay
       blue whatever accent was configured. */
    background: linear-gradient(
      135deg,
      var(--p11-accent),
      color-mix(in oklab, var(--p11-accent), rgb(var(--p11-shade-rgb)) 18%)
    );
    border: none;
    border-radius: var(--radius-full);
    color: rgb(var(--p11-overlay-rgb));
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-medium);
    font-family: var(--font-family);
    cursor: pointer;
    box-shadow: 0 2px 8px color-mix(in srgb, var(--p11-accent) 25%, transparent);
    transition:
      transform var(--transition-spring),
      box-shadow var(--transition-normal);
  }

  .welcome-btn:hover {
    transform: translateY(-1px);
    box-shadow: 0 4px 16px color-mix(in srgb, var(--p11-accent) 35%, transparent);
  }

  .welcome-btn:active {
    transform: translateY(0);
  }
</style>
