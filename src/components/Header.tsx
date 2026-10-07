import { Folder, Moon, Sun, EyeOff } from 'lucide-react';
import { ACCENTS } from '../lib/accents';

interface HeaderProps {
  scanPath: string;
  accent: string;
  setAccent: (accent: string) => void;
  theme: 'light' | 'dark';
  setTheme: (theme: 'light' | 'dark') => void;
  hiddenCount: number;
  onShowHidden: () => void;
  version: string;
}

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');

export default function Header({
  scanPath, accent, setAccent, theme, setTheme, hiddenCount, onShowHidden, version,
}: HeaderProps) {
  return (
    <header className="h-14 border-b border-outline bg-surface-container flex items-center justify-between gap-4 px-5 shrink-0">
      <div className="flex items-center gap-2 min-w-0 text-sm text-on-surface/60" title={scanPath || undefined}>
        <Folder className="w-4 h-4 shrink-0" />
        <span className="font-mono text-xs truncate">{scanPath ? tildePath(scanPath) : 'No folder chosen'}</span>
      </div>

      <div className="flex items-center gap-3 shrink-0">
        {hiddenCount > 0 && (
          <button
            onClick={onShowHidden}
            title="Manage hidden paths in Settings"
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs text-on-surface/60 hover:text-on-surface hover:bg-surface-bright transition-colors"
          >
            <EyeOff className="w-3.5 h-3.5" />
            {hiddenCount} hidden
          </button>
        )}

        <div className="flex items-center gap-1" role="radiogroup" aria-label="Accent color">
          {ACCENTS.map(c => {
            const selected = accent === c.id;
            return (
              <button
                key={c.id}
                role="radio"
                aria-checked={selected}
                aria-label={c.label}
                title={c.label}
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
          aria-label={theme === 'dark' ? 'Use light appearance' : 'Use dark appearance'}
          title={theme === 'dark' ? 'Light appearance' : 'Dark appearance'}
          className="p-1.5 rounded-md text-on-surface/60 hover:text-on-surface hover:bg-surface-bright transition-colors"
        >
          {theme === 'dark' ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
        </button>

        {version && <span className="text-[11px] text-on-surface/40 tabular-nums">v{version}</span>}
      </div>
    </header>
  );
}
