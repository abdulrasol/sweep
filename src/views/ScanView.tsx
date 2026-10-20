import { useState, useEffect, useCallback } from 'react';
import { FolderOpen, ScanSearch } from 'lucide-react';
import { invoke } from '../lib/tauri';
import { PROJECT_MODULES, GLOBAL_MODULES, type ModuleDef } from '../lib/modules';

interface ScanViewProps {
  initialPath: string;
  initialModules: string[];
  onScan: (path: string, modules: string[]) => void;
}

const ALL_MODULES = [...PROJECT_MODULES, ...GLOBAL_MODULES];
const AVAILABLE_IDS = ALL_MODULES.filter(m => !m.soon).map(m => m.id);

function ModuleCard({ mod, on, onToggle }: { mod: ModuleDef; on: boolean; onToggle: () => void }) {
  const Icon = mod.icon;
  return (
    <button
      role="switch"
      aria-checked={on}
      disabled={mod.soon}
      onClick={onToggle}
      className={`text-left flex gap-3 p-3.5 rounded-xl border transition-colors
        ${mod.soon ? 'border-dashed border-outline opacity-60 cursor-not-allowed'
          : on ? 'border-primary/50 bg-primary/[0.05]' : 'border-outline bg-surface-container hover:border-outline-variant'}`}
    >
      <span className={`mt-0.5 shrink-0 transition-opacity ${on || mod.soon ? '' : 'opacity-40'}`} style={{ color: mod.color }}>
        <Icon className="w-5 h-5" />
      </span>
      <span className="flex-1 min-w-0">
        <span className="flex items-center gap-2">
          <span className="text-sm font-medium truncate">{mod.title}</span>
          {mod.review && !mod.soon && (
            <span title="Some items need your confirmation. They are not selected automatically." className="text-[10px] px-1.5 py-px rounded border border-warning/40 text-warning">Review</span>
          )}
          {mod.soon && <span className="text-[10px] px-1.5 py-px rounded bg-surface-bright text-on-surface/60">Coming later</span>}
        </span>
        <span className="block mt-0.5 text-xs text-on-surface/55 leading-snug">{mod.description}</span>
      </span>
      {!mod.soon && (
        <span className={`mt-0.5 w-8 h-[18px] shrink-0 rounded-full p-0.5 transition-colors ${on ? 'bg-primary' : 'bg-outline-variant'}`} aria-hidden>
          <span className={`block w-3.5 h-3.5 rounded-full bg-white shadow-sm transition-transform ${on ? 'translate-x-3.5' : ''}`} />
        </span>
      )}
    </button>
  );
}

export default function ScanView({ initialPath, initialModules, onScan }: ScanViewProps) {
  const [path, setPath] = useState(initialPath);
  const [enabled, setEnabled] = useState<Set<string>>(new Set(initialModules.filter(id => AVAILABLE_IDS.includes(id))));
  const [pickError, setPickError] = useState<string | null>(null);
  const [fdaStatus, setFdaStatus] = useState<string>('unknown');

  const checkAccess = useCallback(async () => {
    try {
      setFdaStatus(await invoke<string>('check_full_disk_access'));
    } catch {
      setFdaStatus('unknown');
    }
  }, []);

  useEffect(() => {
    if (initialPath) setPath(initialPath);
  }, [initialPath]);

  useEffect(() => {
    checkAccess();
    // Re-check when the window regains focus, so the card disappears after access is granted.
    window.addEventListener('focus', checkAccess);
    return () => window.removeEventListener('focus', checkAccess);
  }, [checkAccess]);

  const chooseFolder = async () => {
    try {
      const selected = await invoke<string | null>('select_directory');
      if (selected) setPath(selected);
      setPickError(null);
    } catch (err) {
      setPickError(String(err));
    }
  };

  const toggle = (id: string) =>
    setEnabled(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const allOn = enabled.size === AVAILABLE_IDS.length;
  const canScan = enabled.size > 0 && path.trim() !== '';

  const section = (title: string, hint: string, list: ModuleDef[]) => (
    <section className="space-y-3">
      <div>
        <h3 className="text-sm font-semibold">{title}</h3>
        <p className="text-xs text-on-surface/55">{hint}</p>
      </div>
      <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-2.5">
        {list.map(m => <ModuleCard key={m.id} mod={m} on={enabled.has(m.id)} onToggle={() => toggle(m.id)} />)}
      </div>
    </section>
  );

  return (
    <div className="h-full flex flex-col">
      <div className="flex-1 overflow-y-auto">
        <div className="max-w-5xl mx-auto px-6 py-6 space-y-8">
          {fdaStatus === 'missing' && (
            <section className="border border-warning/40 bg-warning/[0.06] rounded-2xl p-5 space-y-3">
              <div className="space-y-1">
                <h3 className="text-sm font-semibold">Sweep cannot see some protected folders</h3>
                <p className="text-xs text-on-surface/60 leading-relaxed">
                  Without Full Disk Access, macOS hides folders like iPhone backups, the Trash and Safari data, so they may be missing from the results. Allow Sweep in Privacy and Security, then quit and reopen Sweep.
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  onClick={() => { invoke('open_privacy_settings').catch(() => {}); }}
                  className="px-3.5 py-2 rounded-lg border border-outline bg-surface-container text-sm font-medium hover:bg-surface-bright transition-colors"
                >
                  Open Privacy Settings
                </button>
                <button
                  onClick={checkAccess}
                  className="px-3.5 py-2 rounded-lg border border-outline bg-surface-container text-sm font-medium hover:bg-surface-bright transition-colors"
                >
                  Check again
                </button>
              </div>
            </section>
          )}
          <header className="space-y-1.5">
            <h2 className="text-xl font-semibold tracking-tight">Choose what to scan</h2>
            <p className="text-sm text-on-surface/60 max-w-2xl">
              Sweep looks for build output inside your projects folder and for caches left by the tools you turn on.
              Nothing is removed until you review it.
            </p>
          </header>

          <section className="space-y-2">
            <label htmlFor="scan-path" className="text-sm font-semibold">Projects folder</label>
            <div className="flex gap-2">
              <input
                id="scan-path"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="/Users/you/Projects"
                spellCheck={false}
                className="flex-1 min-w-0 bg-surface-container border border-outline rounded-lg px-3 py-2 font-mono text-xs outline-none focus:border-primary/60 placeholder:text-on-surface/30"
              />
              <button
                onClick={chooseFolder}
                className="flex items-center gap-2 px-3.5 py-2 rounded-lg border border-outline bg-surface-container text-sm font-medium hover:bg-surface-bright transition-colors"
              >
                <FolderOpen className="w-4 h-4" />
                Choose…
              </button>
            </div>
            {pickError && <p className="text-xs text-error">Could not open the folder picker: {pickError}</p>}
          </section>

          {section('In your projects', 'Build folders found under the projects folder. Folders with files tracked by git are never removed.', PROJECT_MODULES)}
          {section('On this Mac', 'Caches outside your projects, in your home folder and ~/Library.', GLOBAL_MODULES)}
        </div>
      </div>

      <footer className="border-t border-outline bg-surface-container px-6 py-3.5 shrink-0">
        <div className="max-w-5xl mx-auto flex items-center gap-4">
          <span className="text-sm text-on-surface/60 tabular-nums">
            {enabled.size} of {AVAILABLE_IDS.length} on
          </span>
          <button
            onClick={() => setEnabled(allOn ? new Set() : new Set(AVAILABLE_IDS))}
            className="text-sm text-primary hover:underline"
          >
            {allOn ? 'Turn all off' : 'Turn all on'}
          </button>
          <button
            onClick={() => onScan(path.trim(), Array.from(enabled))}
            disabled={!canScan}
            className="ml-auto flex items-center gap-2 px-5 py-2 rounded-lg bg-on-surface text-surface text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <ScanSearch className="w-4 h-4" />
            Scan
          </button>
        </div>
      </footer>
    </div>
  );
}
