import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { en, type MessageKey } from './en';
import { ar } from './ar';

export type Lang = 'en' | 'ar';

export const LANGUAGES: { id: Lang; label: string }[] = [
  { id: 'en', label: 'English' },
  { id: 'ar', label: 'العربية' },
];

const STORAGE_KEY = 'sweep_lang';
const DICTIONARIES: Record<Lang, Record<string, string>> = { en, ar };

export const isRtl = (lang: Lang) => lang === 'ar';

/** Saved choice, else the browser language, else English. */
export function detectLang(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'en' || saved === 'ar') return saved;
  } catch { /* storage unavailable */ }
  const nav = typeof navigator !== 'undefined' ? navigator.language : 'en';
  return nav.toLowerCase().startsWith('ar') ? 'ar' : 'en';
}

type Vars = Record<string, string | number>;

/** Replaces {name} placeholders. Unknown placeholders are left as written. */
export function interpolate(template: string, vars?: Vars): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(vars[name]) : whole);
}

/** Looks a key up in the language, then English, then returns the fallback or key. */
export function lookup(lang: Lang, key: string, fallback?: string): string {
  return DICTIONARIES[lang][key] ?? en[key as MessageKey] ?? fallback ?? key;
}

/** Plural form of `base`: tries base_<category>, then base_other. */
export function lookupPlural(lang: Lang, base: string, n: number): string {
  const category = new Intl.PluralRules(lang).select(n);
  const dict = DICTIONARIES[lang];
  return dict[`${base}_${category}`] ?? dict[`${base}_other`]
    ?? (en as Record<string, string>)[`${base}_other`] ?? base;
}

interface I18n {
  lang: Lang;
  dir: 'ltr' | 'rtl';
  setLang: (lang: Lang) => void;
  /** Typed message with {placeholders}. */
  t: (key: MessageKey, vars?: Vars) => string;
  /** Message whose key is built at run time (module ids); falls back to `fallback`. */
  td: (key: string, fallback: string, vars?: Vars) => string;
  /** Plural message: keys base_one / base_other (Arabic also zero, two, few, many). {n} is filled in. */
  tn: (base: string, n: number, vars?: Vars) => string;
  /** Number in the current language's digits. */
  num: (n: number) => string;
}

const Ctx = createContext<I18n | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectLang);
  const dir = isRtl(lang) ? 'rtl' : 'ltr';

  useEffect(() => {
    document.documentElement.lang = lang;
    document.documentElement.dir = dir;
  }, [lang, dir]);

  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    try { localStorage.setItem(STORAGE_KEY, next); } catch { /* ignore */ }
  }, []);

  const value = useMemo<I18n>(() => ({
    lang,
    dir,
    setLang,
    t: (key, vars) => interpolate(lookup(lang, key), vars),
    td: (key, fallback, vars) => interpolate(lookup(lang, key, fallback), vars),
    tn: (base, n, vars) => interpolate(lookupPlural(lang, base, n), { n: formatNumber(lang, n), ...vars }),
    num: (n) => formatNumber(lang, n),
  }), [lang, dir, setLang]);

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/** Arabic text keeps Western digits so sizes like "1.2 GB" and paths stay aligned. */
function formatNumber(lang: Lang, n: number): string {
  return new Intl.NumberFormat(lang === 'ar' ? 'ar-u-nu-latn' : 'en').format(n);
}

export function useI18n(): I18n {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error('useI18n must be used inside I18nProvider');
  return ctx;
}

export type { MessageKey };
