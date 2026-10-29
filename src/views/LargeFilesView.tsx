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

const MIN_SIZES_MB = [100, 250, 500, 1000, 2000];

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');
const fileName = (path: string) => path.split('/').filter(Boolean).pop() ?? path;
const parentPath = (path: string) => tildePath(path.split('/').slice(0, -1).join('/') || '/');

export default function LargeFilesView({ scanPath, ignoredPaths, onStartCleaning }: LargeFilesViewProps) {
  // An empty folder means the home folder; the backend resolves it.
  const [folder, setFolder] = useState('');
  const [minMb, setMinMb] = useState(500);
  const [items, setItems] = useState<CleanupItem[] | null>(null);
  const [searchedFolder, setSearchedFolder] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmText, setConfirmText] = useState('');

  const search = async () => {
    setBusy(true);
    setError(null);
    setItems(null);
    setSelected(new Set());
    setConfirmText('');
    try {
      const data = await invoke<CleanupItem[]>('find_large_files', { path: folder, minMb, ignoredPaths });
      setItems(data);
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

  const chosen = (items ?? []).filter(i => selected.has(i.id));
  const chosenBytes = chosen.reduce((sum, i) => sum + i.size_bytes, 0);
  const canStart = chosen.length > 0 && confirmText === 'DELETE';
  const nowMs = Date.now();
  const where = searchedFolder ? tildePath(searchedFolder) : 'your home folder';

  return (
    <div className="max-w-4xl mx-auto px-6 py-8 space-y-5">
      <header className="space-y-1">
        <h2 className="text-2xl font-semibold tracking-tight">Large files</h2>
        <p className="text-sm text-on-surface/60">
          Finds the biggest files you may have forgotten, like videos, archives and disk images.
          These are your own files, so Sweep only moves them to the Trash and asks you to confirm.
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
            Bigger than
            <select
              value={minMb}
              onChange={(e) => setMinMb(Number(e.target.value))}
              className="bg-surface-dim border border-outline rounded-lg px-3 py-1.5 text-sm outline-none focus:border-primary/60"
            >
              {MIN_SIZES_MB.map(mb => (
                <option key={mb} value={mb}>{mb >= 1000 ? `${mb / 1000} GB` : `${mb} MB`}</option>
              ))}
            </select>
          </label>
          {busy ? (
            <>
              <span className="flex items-center gap-2 text-sm text-on-surface/60">
                <Loader2 className="w-4 h-4 animate-spin" /> Searching…
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
              Find large files
            </button>
          )}
        </div>
        <p className="text-xs text-on-surface/45">
          Skipped on purpose: hidden folders, Library, build and dependency folders, app and library bundles,
          and files with another hard link.
        </p>
      </section>

      {error && <p className="text-sm text-error">{error}</p>}

      {items !== null && items.length === 0 && (
        <p className="text-sm text-on-surface/60">
          No files bigger than {minMb >= 1000 ? `${minMb / 1000} GB` : `${minMb} MB`} in {where}.
        </p>
      )}

      {items !== null && items.length > 0 && (
        <section className="bg-surface-container border border-outline rounded-2xl overflow-hidden">
          <div className="px-5 py-3 border-b border-outline text-sm text-on-surface/60 tabular-nums">
            {items.length} {items.length === 1 ? 'file' : 'files'} in {where} · {formatBytes(items.reduce((s, i) => s + i.size_bytes, 0))}
          </div>
          <ul className="divide-y divide-outline">
            {items.map(item => (
              <li key={item.id} className="flex items-center gap-3 px-5 py-3">
                <input
                  type="checkbox"
                  checked={selected.has(item.id)}
                  onChange={() => toggle(item.id)}
                  aria-label={`Select ${fileName(item.path)}`}
                  className="w-4 h-4 accent-[var(--color-primary)] shrink-0"
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
                </div>
                <button
                  onClick={() => reveal(item.id)}
                  className="text-xs text-on-surface/60 hover:text-on-surface shrink-0"
                >
                  Show in Finder
                </button>
                <span className="w-24 text-right text-sm tabular-nums shrink-0">{formatBytes(item.size_bytes)}</span>
              </li>
            ))}
          </ul>
        </section>
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
