<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { X, Minus, Square } from '@lucide/svelte';

  async function handleClose(): Promise<void> {
    await getCurrentWindow().close();
  }

  async function handleMinimize(): Promise<void> {
    await getCurrentWindow().minimize();
  }

  async function handleMaximize(): Promise<void> {
    const window = getCurrentWindow();
    if (await window.isMaximized()) {
      await window.unmaximize();
    } else {
      await window.maximize();
    }
  }
</script>

<div class="window-controls">
  <button class="control-btn minimize" onclick={handleMinimize} aria-label="Minimize">
    <Minus size={14} />
  </button>
  <button class="control-btn maximize" onclick={handleMaximize} aria-label="Maximize">
    <Square size={12} />
  </button>
  <button class="control-btn close" onclick={handleClose} aria-label="Close">
    <X size={14} />
  </button>
</div>

<style>
  .window-controls {
    display: flex;
    align-items: center;
    gap: 2px;
    -webkit-app-region: no-drag;
  }

  .control-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background-color: transparent;
    color: var(--p11-text-muted);
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .control-btn:hover {
    background-color: var(--p11-bg-elevated);
  }

  .control-btn.close:hover {
    background-color: var(--p11-error);
    color: rgb(var(--p11-overlay-rgb));
  }
</style>
