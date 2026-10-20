const DAY_SECONDS = 86_400;

/**
 * Human "last active" text for a project item. Pure so the caller controls the
 * clock and the result is easy to reason about.
 */
export function formatAge(lastActive: number, nowMs: number): string {
  const days = Math.max(0, Math.floor((nowMs / 1000 - lastActive) / DAY_SECONDS));
  if (days < 1) return 'today';
  if (days < 30) return days === 1 ? '1 day ago' : `${days} days ago`;
  if (days < 365) {
    const months = Math.floor(days / 30);
    return months === 1 ? '1 month ago' : `${months} months ago`;
  }
  const years = Math.floor(days / 365);
  return years === 1 ? '1 year ago' : `${years} years ago`;
}
