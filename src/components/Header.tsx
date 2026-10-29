import { Folder, Moon, Sun, EyeOff, ArrowUpCircle } from 'lucide-react';
import { ACCENTS } from '../lib/accents';
import { useI18n } from '../i18n';

interface HeaderProps {
  scanPath: string;
  accent: string;
  setAccent: (accent: string) => void;
  theme: 'light' | 'dark';
  setTheme: (theme: 'light' | 'dark') => void;
  hiddenCount: number;
  onShowHidden: () => void;
  version: string;
  /** Newer version found by the update check, or null. */
  updateVersion: string | null;
  onShowUpdate: () => void;
}

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');

export default function Header({
  scanPath, accent, setAccent, theme, setTheme, hiddenCount, onShowHidden, version,
  updateVersion, onShowUpdate,
}: HeaderProps) {
  const { t, td } = useI18n();
  return (
    <header className="h-14 border-b border-outline bg-surface-container flex items-center justify-between gap-4 px-5 shrink-0">
      <div className="flex items-center gap-2 min-w-0 text-sm text-on-surface/60" title={scanPath || undefined}>
        <Folder className="w-4 h-4 shrink-0" />
        <span className="font-mono text-xs truncate">{scanPath ? tildePath(scanPath) : t('header.noFolder')}</span>
      </div>

      <div className="flex items-center gap-3 shrink-0">
        {hiddenCount > 0 && (
          <button
            onClick={onShowHidden}
            title={t('header.hiddenTitle')}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs text-on-surface/60 hover:text-on-surface hover:bg-surface-bright transition-colors"
          >
            <EyeOff className="w-3.5 h-3.5" />
            {t('header.hidden', { n: hiddenCount })}
          </button>
        )}

        <div className="flex items-center gap-1" role="radiogroup" aria-label={t('header.accent')}>
          {ACCENTS.map(c => {
            const selected = accent === c.id;
            return (
              <button
                key={c.id}
                role="radio"
                aria-checked={selected}
                aria-label={td(`accent.${c.id}`, c.label)}
                title={td(`accent.${c.id}`, c.label)}
                onClick={() => setAccent(c.id)}
                className="w-6 h-6 grid place-items-center rounded-full"
              >
                <span
                  className={`block rounded-full transition-all ${selected ? 'w-3.5 h-3.5 ring-2 ring-offset-2 ring-offset-surface-container' : 'w-2.5 h-2.5 opacity-60 hover:opacity-100'}`}
                  style={{ backgroundColor: c.hex, ...(selected ? { ['--tw-ring-color' as string]: c.hex } : {}) }}
                />
              </button>
            );
          })}
        </div>

        <button
          onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
          aria-label={theme === 'dark' ? t('header.useLight') : t('header.useDark')}
          title={theme === 'dark' ? t('header.light') : t('header.dark')}
          className="p-1.5 rounded-md text-on-surface/60 hover:text-on-surface hover:bg-surface-bright transition-colors"
        >
          {theme === 'dark' ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
        </button>

        {updateVersion ? (
          <button
            onClick={onShowUpdate}
            title={t('header.updateAvailable', { version: updateVersion })}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-primary/15 text-primary text-xs font-medium hover:bg-primary/25 transition-colors"
          >
            <ArrowUpCircle className="w-3.5 h-3.5" />
            {t('header.updateTo', { version: updateVersion })}
          </button>
        ) : (
          version && <span className="text-[11px] text-on-surface/40 tabular-nums" dir="ltr">v{version}</span>
        )}
      </div>
    </header>
  );
}
