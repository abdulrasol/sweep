import { useState } from 'react';
import { FolderOpen, Home, Loader2, ShieldAlert } from 'lucide-react';
import { invoke, formatBytes } from '../lib/tauri';
import { formatAge } from '../lib/age';

interface LargeFilesViewProps {
  /** The folder chosen on the Scan screen, offered as a shortcut. */
  scanPath: string;
  ignoredPaths: string[];
  onStartCleaning: (items: CleanupItem[]) => void;
}

type Mode = 'large' | 'duplicates';

const MODES: { id: Mode; label: string; command: string; sizes: number[]; defaultMb: number; intro: string }[] = [
  {
    id: 'large',
    label: 'Large files',
    command: 'find_large_files',
    sizes: [100, 250, 500, 1000, 2000],
    defaultMb: 500,
    intro: 'Finds the biggest files you may have forgotten, like videos, archives and disk images.',
  },
  {
    id: 'duplicates',
    label: 'Duplicates',
    command: 'find_duplicates',
    sizes: [10, 50, 100, 250, 500],
    defaultMb: 50,
    intro: 'Finds files with exactly the same contents, so you can keep one copy and remove the rest.',
  },
];

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');
const fileName = (path: string) => path.split('/').filter(Boolean).pop() ?? path;
const parentPath = (path: string) => tildePath(path.split('/').slice(0, -1).join('/') || '/');
const sizeLabel = (mb: number) => (mb >= 1000 ? `${mb / 1000} GB` : `${mb} MB`);

export default function LargeFilesView({ scanPath, ignoredPaths, onStartCleaning }: LargeFilesViewProps) {
  const [mode, setMode] = useState<Mode>('large');
  // An empty folder means the home folder; the backend resolves it.
  const [folder, setFolder] = useState('');
  const [minMb, setMinMb] = useState(500);
  const [files, setFiles] = useState<CleanupItem[] | null>(null);
  const [groups, setGroups] = useState<DuplicateGroup[] | null>(null);
  const [searchedFolder, setSearchedFolder] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmText, setConfirmText] = useState('');

  const current = MODES.find(m => m.id === mode) ?? MODES[0];

  const reset = () => {
    setFiles(null);
    setGroups(null);
    setSelected(new Set());
    setConfirmText('');
    setError(null);
  };

  const switchMode = (next: Mode) => {
    if (busy || next === mode) return;
    const def = MODES.find(m => m.id === next) ?? MODES[0];
    setMode(next);
    setMinMb(def.defaultMb);
    reset();
  };

  const search = async () => {
    setBusy(true);
    reset();
    try {
      const args = { path: folder, minMb, ignoredPaths };
      if (mode === 'large') {
        setFiles(await invoke<CleanupItem[]>(current.command, args));
      } else {
        setGroups(await invoke<DuplicateGroup[]>(current.command, args));
      }
      setSearchedFolder(folder);
    } catch (err) {
      const message = typeof err === 'string' ? err : String(err);
      if (message !== 'Search cancelled.') setError(message);
    } finally {
      setBusy(false);
    }
  };

  const cancel = () => {
    invoke('cancel_scan').catch(() => {});
  };

  const chooseFolder = async () => {
    try {
      const chosen = await invoke<string | null>('select_directory');
      if (chosen) setFolder(chosen);
    } catch {
      // The dialog was closed or failed; keep the current folder.
    }
  };

  const toggle = (id: string) =>
    setSelected(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const reveal = (id: string) => {
    invoke('reveal_item', { id }).catch(() => {});
  };

  /** Select every copy except the oldest one of each set. */
  const selectExtras = () => {
    setSelected(new Set((groups ?? []).flatMap(g => g.items.slice(1).map(i => i.id))));
  };

  /** The last unselected copy of a set cannot be selected: one copy always stays. */
  const isLastCopy = (group: DuplicateGroup, item: CleanupItem) =>
    !selected.has(item.id) && group.items.every(other => other.id === item.id || selected.has(other.id));

  const flat = mode === 'large' ? files ?? [] : (groups ?? []).flatMap(g => g.items);
  const chosen = flat.filter(i => selected.has(i.id));
  const chosenBytes = chosen.reduce((sum, i) => sum + i.size_bytes, 0);
  const canStart = chosen.length > 0 && confirmText === 'DELETE';
  const nowMs = Date.now();
  const where = searchedFolder ? tildePath(searchedFolder) : 'your home folder';
  const wastedTotal = (groups ?? []).reduce((sum, g) => sum + g.wasted, 0);

  const renderRow = (item: CleanupItem, lastCopy: boolean) => (
    <li key={item.id} className="flex items-center gap-3 px-5 py-3">
      <input
        type="checkbox"
        checked={selected.has(item.id)}
        disabled={lastCopy}
        onChange={() => toggle(item.id)}
        aria-label={`Select ${fileName(item.path)}`}
        title={lastCopy ? 'Keep at least one copy' : undefined}
        className="w-4 h-4 accent-[var(--color-primary)] shrink-0 disabled:opacity-40"
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-sm font-medium truncate" title={item.path}>{fileName(item.path)}</span>
          <span className="text-[11px] px-1.5 py-0.5 rounded bg-surface-bright text-on-surface/60 shrink-0">{item.file_type}</span>
        </div>
        <div className="text-xs text-on-surface/50 truncate font-mono" title={item.path}>{parentPath(item.path)}</div>
        {item.last_active !== null && (
          <div className="text-[11px] text-on-surface/45">Modified {formatAge(item.last_active, nowMs)}</div>
        )}
        {item.description.includes('syncs to the cloud') && (
          <div className="text-[11px] text-warning">In a cloud-synced folder: removing it may remove it online too.</div>
        )}
      </div>
      <button onClick={() => reveal(item.id)} className="text-xs text-on-surface/60 hover:text-on-surface shrink-0">
        Show in Finder
      </button>
      <span className="w-24 text-right text-sm tabular-nums shrink-0">{formatBytes(item.size_bytes)}</span>
    </li>
  );

  return (
    <div className="max-w-4xl mx-auto px-6 py-8 space-y-5">
      <header className="space-y-3">
        <h2 className="text-2xl font-semibold tracking-tight">Large files and duplicates</h2>
        <div className="inline-flex rounded-lg border border-outline p-0.5" role="tablist">
          {MODES.map(m => (
            <button
              key={m.id}
              role="tab"
              aria-selected={mode === m.id}
              onClick={() => switchMode(m.id)}
              disabled={busy}
              className={`px-3 py-1.5 rounded-md text-sm transition-colors ${mode === m.id ? 'bg-surface-bright font-semibold' : 'text-on-surface/60 hover:text-on-surface'}`}
            >
              {m.label}
            </button>
          ))}
        </div>
        <p className="text-sm text-on-surface/60">
          {current.intro} These are your own files, so Sweep only moves them to the Trash and asks you to confirm.
        </p>
      </header>

      <section className="bg-surface-container border border-outline rounded-2xl p-5 space-y-4">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-xs text-on-surface/50 mr-1">Search in</span>
          <button
            onClick={() => setFolder('')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg border text-sm ${folder === '' ? 'border-primary text-primary' : 'border-outline text-on-surface/70 hover:text-on-surface'}`}
          >
            <Home className="w-3.5 h-3.5" /> Home folder
          </button>
          {scanPath && (
            <button
              onClick={() => setFolder(scanPath)}
              title={scanPath}
              className={`px-3 py-1.5 rounded-lg border text-sm ${folder === scanPath ? 'border-primary text-primary' : 'border-outline text-on-surface/70 hover:text-on-surface'}`}
            >
              Scan folder
            </button>
          )}
          <button
            onClick={chooseFolder}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-outline text-sm text-on-surface/70 hover:text-on-surface"
          >
            <FolderOpen className="w-3.5 h-3.5" /> Choose folder…
          </button>
        </div>
        {folder !== '' && folder !== scanPath && (
          <p className="text-xs text-on-surface/50 font-mono truncate" title={folder}>{tildePath(folder)}</p>
        )}

        <div className="flex flex-wrap items-center gap-3">
          <label className="flex items-center gap-2 text-sm text-on-surface/70">
            {mode === 'large' ? 'Bigger than' : 'Files bigger than'}
            <select
              value={minMb}
              onChange={(e) => setMinMb(Number(e.target.value))}
              className="bg-surface-dim border border-outline rounded-lg px-3 py-1.5 text-sm outline-none focus:border-primary/60"
            >
              {current.sizes.map(mb => (
                <option key={mb} value={mb}>{sizeLabel(mb)}</option>
              ))}
            </select>
          </label>
          {busy ? (
            <>
              <span className="flex items-center gap-2 text-sm text-on-surface/60">
                <Loader2 className="w-4 h-4 animate-spin" /> {mode === 'large' ? 'Searching…' : 'Searching and comparing files…'}
              </span>
              <button onClick={cancel} className="px-3 py-1.5 rounded-lg border border-outline text-sm text-on-surface/70 hover:text-on-surface">
                Cancel
              </button>
            </>
          ) : (
            <button
              onClick={search}
              className="px-4 py-2 bg-on-surface text-surface rounded-lg font-medium text-sm hover:opacity-90 active:scale-[0.98] transition"
            >
              {mode === 'large' ? 'Find large files' : 'Find duplicates'}
            </button>
          )}
        </div>
        <p className="text-xs text-on-surface/45">
          Skipped on purpose: hidden folders, Library, build, cache and dependency folders, app and library bundles,
          and files with another hard link.
        </p>
      </section>

      {error && <p className="text-sm text-error">{error}</p>}

      {mode === 'large' && files !== null && files.length === 0 && (
        <p className="text-sm text-on-surface/60">No files bigger than {sizeLabel(minMb)} in {where}.</p>
      )}
      {mode === 'duplicates' && groups !== null && groups.length === 0 && (
        <p className="text-sm text-on-surface/60">No identical files bigger than {sizeLabel(minMb)} in {where}.</p>
      )}

      {mode === 'large' && files !== null && files.length > 0 && (
        <section className="bg-surface-container border border-outline rounded-2xl overflow-hidden">
          <div className="px-5 py-3 border-b border-outline text-sm text-on-surface/60 tabular-nums">
            {files.length} {files.length === 1 ? 'file' : 'files'} in {where} · {formatBytes(files.reduce((s, i) => s + i.size_bytes, 0))}
          </div>
          <ul className="divide-y divide-outline">{files.map(item => renderRow(item, false))}</ul>
        </section>
      )}

      {mode === 'duplicates' && groups !== null && groups.length > 0 && (
        <>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-sm text-on-surface/60 tabular-nums">
              {groups.length} {groups.length === 1 ? 'set' : 'sets'} in {where} · {formatBytes(wastedTotal)} can be freed
            </p>
            <button
              onClick={selectExtras}
              className="px-3 py-1.5 rounded-lg border border-outline text-sm text-on-surface/70 hover:text-on-surface"
            >
              Keep the oldest, select the rest
            </button>
          </div>
          {groups.map(group => (
            <section key={group.items[0].id} className="bg-surface-container border border-outline rounded-2xl overflow-hidden">
              <div className="px-5 py-3 border-b border-outline text-sm text-on-surface/60 tabular-nums">
                {group.items.length} identical copies · {formatBytes(group.size_each)} each · {formatBytes(group.wasted)} can be freed
              </div>
              <ul className="divide-y divide-outline">
                {group.items.map(item => renderRow(item, isLastCopy(group, item)))}
              </ul>
            </section>
          ))}
        </>
      )}

      {chosen.length > 0 && (
        <section className="sticky bottom-4 bg-surface-container border border-outline rounded-2xl px-5 py-4 flex flex-wrap items-center justify-between gap-4 shadow-lg">
          <div className="text-sm tabular-nums">
            <span className="font-semibold">{chosen.length} selected</span>
            <span className="text-on-surface/60"> · {formatBytes(chosenBytes)}. They move to the Trash; empty it to free the space.</span>
          </div>
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-error/40 bg-error/10 text-xs">
              <ShieldAlert className="w-4 h-4 text-error shrink-0" />
              <span className="text-error font-medium hidden sm:inline">Type DELETE to confirm:</span>
              <input
                value={confirmText}
                onChange={(e) => setConfirmText(e.target.value)}
                placeholder="DELETE"
                aria-label="Type DELETE to confirm"
                className="w-28 bg-surface border border-error/40 rounded-md px-2 py-1 text-xs font-mono outline-none focus:border-error text-error placeholder:text-error/40"
              />
            </div>
            <button
              onClick={() => onStartCleaning(chosen)}
              disabled={!canStart}
              className="px-5 py-2.5 bg-on-surface text-surface rounded-lg font-medium text-sm hover:opacity-90 active:scale-[0.98] transition disabled:opacity-40 disabled:cursor-not-allowed"
            >
              Move to Trash
            </button>
          </div>
        </section>
      )}
    </div>
  );
}
