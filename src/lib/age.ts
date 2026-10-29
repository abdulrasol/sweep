import type { MessageKey } from '../i18n';

const DAY_SECONDS = 86_400;

/** The two translators formatAge needs; the i18n hook provides both. */
interface AgeText {
  t: (key: MessageKey) => string;
  tn: (base: string, n: number) => string;
}

/**
 * Human "last active" text for a project item. Pure so the caller controls the
 * clock and the result is easy to reason about.
 */
export function formatAge(lastActive: number, nowMs: number, text: AgeText): string {
  const days = Math.max(0, Math.floor((nowMs / 1000 - lastActive) / DAY_SECONDS));
  if (days < 1) return text.t('age.today');
  if (days < 30) return text.tn('age.days', days);
  if (days < 365) return text.tn('age.months', Math.floor(days / 30));
  return text.tn('age.years', Math.floor(days / 365));
}
