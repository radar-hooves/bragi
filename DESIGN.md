---
name: Bragi
description: Desktop music player built on Tauri v2 + Svelte 5; offline-first, data-sovereign.
---

# Design

Bragi is an ordinary consumer of the public `@poodle64/design-tokens` and `@poodle64/ui` packages. Its near-black, dark-only look is expressed as `--ds-color-*` overrides in `src/app.css` (primary, ring, background, foreground, surface-1..3, muted-foreground, border, destructive), the override point the packages are designed for. No component is forked or vendored; every shadcn-style primitive is imported from `@poodle64/ui/<name>`.

## Colour

`+layout.svelte` forces dark mode with no user-facing toggle, so the light `:root` values exist for completeness only. The dark surface ladder is anchored at `oklch(0.200 0 0)` with the Bragi Blue accent at `oklch(0.647 0.152 257.2)`. The hover/subtle `--accent` token points at the `--p11-accent-bg` translucent-blue tint rather than the package's neutral default, which keeps the blue-glow hover treatment. It is the one deviation from the package's alias mapping, made by redeclaring `--accent` after importing `@poodle64/ui/styles.css`.

## Settings chrome

The Settings page uses a separate `--p11-*` token layer (`src/lib/styles/tokens.css`). It sits alongside the shared tokens rather than inside them, so it is neither a fork of nor an exception to the shared packages.

## Typography and radius

Typography uses the system stack (`-apple-system, BlinkMacSystemFont, Inter, Segoe UI, Roboto`) via `src/lib/styles/tokens.css`. Border radius resolves through the package's `--ds-radius-lg` (`0.625rem`).

## Rule

Do not invent values that differ from `@poodle64/design-tokens` without a recorded reason, and do not vendor a private copy of a component the package ships.
