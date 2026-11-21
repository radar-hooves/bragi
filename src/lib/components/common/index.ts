// EmptyState / ErrorState / LoadingState come from @poodle64/ui, per
// sveltekit-frontend.md §Common Components — the package owns their prop
// contracts. They were local copies until 25/08/2026; a vendored copy cannot
// receive an upstream fix, and the shipped ones carry the a11y roles
// (role="alert"/"status", aria-live) the copies had dropped. Re-exported here
// so call sites keep this repo's one barrel import style.
export { EmptyState } from '@poodle64/ui/empty-state';
export { ErrorState } from '@poodle64/ui/error-state';
export { LoadingState } from '@poodle64/ui/loading-state';
export { default as Spinner } from './Spinner.svelte';
export { default as StatusBar, type StatusBarData } from './StatusBar.svelte';
export { default as SyncStatusIndicator } from './SyncStatusIndicator.svelte';
export { default as WelcomeScreen } from './WelcomeScreen.svelte';
