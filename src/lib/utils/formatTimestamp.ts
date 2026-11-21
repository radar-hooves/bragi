// Canonical timestamp formatting for Bragi.
// All timestamps throughout the app should use these functions.

/**
 * Format a Unix timestamp (milliseconds) as DD/M/YYYY HH:MM.
 *
 * Australian date format, 24-hour time, em space separator for readability.
 * Month is not zero-padded; year is four digits.
 */
export function formatTimestamp(ms: number): string {
  const d = new Date(ms);
  const day = String(d.getDate());
  const month = String(d.getMonth() + 1);
  const year = d.getFullYear();
  const hours = String(d.getHours()).padStart(2, '0');
  const minutes = String(d.getMinutes()).padStart(2, '0');
  return `${day}/${month}/${year}\u2003${hours}:${minutes}`;
}

/**
 * Format a Unix timestamp (seconds) as a full date-time string.
 *
 * Convenience wrapper for timestamps stored as seconds (e.g. SQLite).
 */
export function formatTimestampSecs(secs: number): string {
  return formatTimestamp(secs * 1000);
}

/**
 * Format a Unix timestamp (milliseconds) as a relative/absolute hybrid.
 *
 * - Under 1 hour: "Xm ago"
 * - 1-24 hours: "Xh ago"
 * - 1-7 days: "Xd ago"
 * - Over 7 days: absolute date (DD/MM/YYYY)
 */
export function formatRelativeTimestamp(ms: number): string {
  const now = Date.now();
  const diffMs = now - ms;
  const diffMinutes = Math.floor(diffMs / 60_000);
  const diffHours = Math.floor(diffMs / 3_600_000);
  const diffDays = Math.floor(diffMs / 86_400_000);

  if (diffMinutes < 1) return 'Just now';
  if (diffMinutes < 60) return `${diffMinutes}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays <= 7) return `${diffDays}d ago`;

  const d = new Date(ms);
  const day = String(d.getDate());
  const month = String(d.getMonth() + 1);
  const year = d.getFullYear();
  return `${day}/${month}/${year}`;
}
