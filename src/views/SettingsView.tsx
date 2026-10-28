import type { ReactNode } from 'react';
import { Moon, Sun, Eye } from 'lucide-react';
import { ACCENTS } from '../lib/accents';
import UpdatePanel from '../components/UpdatePanel';
import type { UpdateState } from '../lib/updates';
import { SEARCH_DEPTHS } from '../lib/scanDepth';

interface SettingsViewProps {
  accent: string;
  onAccentChange: (accent: string) => void;
  theme: 'light' | 'dark';
  onThemeChange: (theme: 'light' | 'dark') => void;
  deleteMode: 'trash' | 'permanent';
  onDeleteModeChange: (mode: 'trash' | 'permanent') => void;
  maxDepth: number;
  onMaxDepthChange: (depth: number) => void;
  ignoredPaths: string[];
  onUnhide: (path: string) => void;
  onUnhideAll: () => void;
  autoUpdateCheck: boolean;
  onAutoUpdateCheckChange: (on: boolean) => void;
  update: UpdateState;
  onCheckForUpdates: () => void;
}

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');

function Row({ title, hint, children }: { title: string; hint: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-4 px-5 py-4">
      <div className="space-y-0.5 min-w-[200px]">
        <p className="text-sm font-medium">{title}</p>
        <p className="text-xs text-on-surface/55">{hint}</p>
      </div>
      {children}
    </div>
  );
}

export default function SettingsView({
  accent, onAccentChange, theme, onThemeChange,
  deleteMode, onDeleteModeChange, maxDepth, onMaxDepthChange,
  ignoredPaths, onUnhide, onUnhideAll,
  autoUpdateCheck, onAutoUpdateCheckChange, update, onCheckForUpdates,
}: SettingsViewProps) {
  return (
    <div className="max-w-3xl mx-auto px-6 py-6 space-y-6">
      <header className="space-y-1">
        <h2 className="text-xl font-semibold tracking-tight">Settings</h2>
        <p className="text-sm text-on-surface/60">Changes are saved as you make them.</p>
      </header>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">Appearance</h3>
        <div className="bg-surface-container border border-outline rounded-xl divide-y divide-outline">
          <Row title="Theme" hint="Light or dark window.">
            <div className="flex rounded-lg border border-outline p-0.5 bg-surface-dim" role="radiogroup" aria-label="Theme">
              {(['light', 'dark'] as const).map(t => (
                <button
                  key={t}
                  role="radio"
                  aria-checked={theme === t}
                  onClick={() => onThemeChange(t)}
                  className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm transition-colors
                    ${theme === t ? 'bg-surface-container shadow-sm font-medium' : 'text-on-surface/60 hover:text-on-surface'}`}
                >
                  {t === 'light' ? <Sun className="w-3.5 h-3.5" /> : <Moon className="w-3.5 h-3.5" />}
                  {t === 'light' ? 'Light' : 'Dark'}
                </button>
              ))}
            </div>
          </Row>
          <Row title="Accent color" hint="Used for selection, progress and the active step.">
            <div className="flex gap-1.5" role="radiogroup" aria-label="Accent color">
              {ACCENTS.map(c => (
                <button
                  key={c.id}
                  role="radio"
                  aria-checked={accent === c.id}
                  onClick={() => onAccentChange(c.id)}
                  className={`flex items-center gap-2 px-2.5 py-1.5 rounded-lg border text-sm transition-colors
                    ${accent === c.id ? 'border-on-surface/40 font-medium' : 'border-outline text-on-surface/60 hover:text-on-surface'}`}
                >
                  <span className="w-3 h-3 rounded-full" style={{ backgroundColor: c.hex }} />
                  {c.label}
                </button>
              ))}
            </div>
          </Row>
        </div>
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">Scanning</h3>
        <div className="bg-surface-container border border-outline rounded-xl divide-y divide-outline">
          <Row
            title="Search depth"
            hint="How many folder levels below the projects folder Sweep searches. Deeper finds projects nested further down, but takes longer."
          >
            <select
              aria-label="Search depth"
              value={maxDepth}
              onChange={(e) => onMaxDepthChange(Number(e.target.value))}
              className="bg-surface-dim border border-outline rounded-lg px-3 py-1.5 text-sm outline-none focus:border-primary/60"
            >
              {SEARCH_DEPTHS.map(depth => (
                <option key={depth} value={depth}>{depth} levels</option>
              ))}
            </select>
          </Row>
        </div>
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">Removal</h3>
        <div className="bg-surface-container border border-outline rounded-xl divide-y divide-outline">
          <Row
            title="When removing files"
            hint="The Trash lets you undo, but it frees no space until you empty it. Items over 5 GB, items already in the Trash and items removed by their own tool (Docker, Homebrew, Ollama) are always removed for good."
          >
            <div className="flex rounded-lg border border-outline p-0.5 bg-surface-dim" role="radiogroup" aria-label="When removing files">
              {(['trash', 'permanent'] as const).map(mode => (
                <button
                  key={mode}
                  role="radio"
                  aria-checked={deleteMode === mode}
                  onClick={() => onDeleteModeChange(mode)}
                  className={`px-3 py-1.5 rounded-md text-sm transition-colors
                    ${deleteMode === mode ? 'bg-surface-container shadow-sm font-medium' : 'text-on-surface/60 hover:text-on-surface'}`}
                >
                  {mode === 'trash' ? 'Move to Trash' : 'Delete permanently'}
                </button>
              ))}
            </div>
          </Row>
        </div>
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">Updates</h3>
        <div className="bg-surface-container border border-outline rounded-xl divide-y divide-outline">
          <Row title="Check at launch" hint="Ask sweep.subultech.top for the latest version each time Sweep opens. Nothing about your files is sent.">
            <div className="flex rounded-lg border border-outline p-0.5 bg-surface-dim" role="radiogroup" aria-label="Check for updates at launch">
              {([true, false] as const).map(on => (
                <button
                  key={String(on)}
                  role="radio"
                  aria-checked={autoUpdateCheck === on}
                  onClick={() => onAutoUpdateCheckChange(on)}
                  className={`px-3 py-1.5 rounded-md text-sm transition-colors
                    ${autoUpdateCheck === on ? 'bg-surface-container shadow-sm font-medium' : 'text-on-surface/60 hover:text-on-surface'}`}
                >
                  {on ? 'On' : 'Off'}
                </button>
              ))}
            </div>
          </Row>
          <div className="px-5 py-4">
            <UpdatePanel update={update} onCheck={onCheckForUpdates} />
          </div>
        </div>
      </section>

      <section className="space-y-2">
        <div className="flex items-end justify-between gap-4">
          <div>
            <h3 className="text-sm font-semibold">Hidden paths</h3>
            <p className="text-xs text-on-surface/55">Paths you chose to hide in Review. Sweep skips them on every scan.</p>
          </div>
          {ignoredPaths.length > 1 && (
            <button onClick={onUnhideAll} className="text-sm text-primary hover:underline shrink-0">Show all again</button>
          )}
        </div>
        <div className="bg-surface-container border border-outline rounded-xl">
          {ignoredPaths.length === 0 ? (
            <p className="px-5 py-4 text-sm text-on-surface/55">
              Nothing is hidden. Use the hide button on an item in Review to skip it in future scans.
            </p>
          ) : (
            <ul className="divide-y divide-outline">
              {ignoredPaths.map(p => (
                <li key={p} className="flex items-center gap-3 px-5 py-2.5">
                  <span className="flex-1 min-w-0 font-mono text-xs truncate" title={p}>{tildePath(p)}</span>
                  <button
                    onClick={() => onUnhide(p)}
                    className="flex items-center gap-1.5 text-xs text-on-surface/60 hover:text-on-surface shrink-0"
                  >
                    <Eye className="w-3.5 h-3.5" />
                    Show again
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </section>
    </div>
  );
}
