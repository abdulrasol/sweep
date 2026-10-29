import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ArrowUp, Check, File as FileIcon, Folder, FolderOpen, Home, Loader2, RefreshCw, ShieldAlert } from 'lucide-react';
import { invoke, formatBytes } from '../lib/tauri';
import { squarify, type Rect } from '../lib/treemap';
import { cacheMap, clearDiskMapCache, getCachedMap } from '../lib/diskMapCache';
import { useI18n } from '../i18n';

interface DiskMapViewProps {
  /** The folder chosen on the Scan screen, offered as a shortcut. */
  scanPath: string;
  onStartCleaning: (items: CleanupItem[]) => void;
}

const MAP_HEIGHT = 440;
/** Squares smaller than this show no text. */
const MIN_LABEL_W = 78;
const MIN_LABEL_H = 40;

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');
const lastSegment = (path: string) => path.split('/').filter(Boolean).pop() ?? path;

interface Cell {
  key: string;
  entry: MapEntry | null;
  label: string;
  size: number;
  rect: Rect;
}

export default function DiskMapView({ scanPath, onStartCleaning }: DiskMapViewProps) {
  const { t, tn } = useI18n();
  const [map, setMap] = useState<DiskMap | null>(null);
  // Absolute paths from the starting folder down to the folder on screen.
  const [trail, setTrail] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [fromCache, setFromCache] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [prepareError, setPrepareError] = useState<string | null>(null);
  const [width, setWidth] = useState(0);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmText, setConfirmText] = useState('');
  const [preparing, setPreparing] = useState(false);

  const boxRef = useRef<HTMLDivElement | null>(null);
  // Only the newest request may update the screen.
  const requestId = useRef(0);

  const show = useCallback((result: DiskMap, nextTrail: string[], cached: boolean) => {
    setMap(result);
    setTrail(nextTrail);
    setFromCache(cached);
    setSelected(new Set());
    setConfirmText('');
  }, []);

  const load = useCallback(async (
    path: string,
    nextTrail: (resolved: string) => string[],
    force = false,
  ) => {
    const id = ++requestId.current;
    const cached = force ? undefined : getCachedMap(path);
    if (cached) {
      setBusy(false);
      setError(null);
      show(cached, nextTrail(cached.path), true);
      return;
    }
    // Stops a measurement that is still running for an earlier folder.
    await invoke('cancel_scan').catch(() => {});
    if (id !== requestId.current) return;
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<DiskMap>('get_disk_map', { path });
      if (id !== requestId.current) return;
      cacheMap(path, result);
      show(result, nextTrail(result.path), false);
    } catch (err) {
      if (id !== requestId.current) return;
      const message = typeof err === 'string' ? err : String(err);
      if (message !== 'Search cancelled.') setError(message);
    } finally {
      if (id === requestId.current) setBusy(false);
    }
  }, [show]);

  const start = (path: string) => load(path, resolved => [resolved]);
  const open = (path: string) => load(path, () => [...trail, path]);
  const goTo = (index: number) => load(trail[index], () => trail.slice(0, index + 1));
  const refresh = () => {
    if (!map) return;
    clearDiskMapCache();
    load(map.path, () => trail, true);
  };

  const cancel = () => {
    requestId.current++;
    setBusy(false);
    invoke('cancel_scan').catch(() => {});
  };

  const chooseFolder = async () => {
    try {
      const chosen = await invoke<string | null>('select_directory');
      if (chosen) start(chosen);
    } catch {
      // The dialog was closed or failed; keep the current map.
    }
  };

  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const observer = new ResizeObserver(entries => setWidth(Math.floor(entries[0].contentRect.width)));
    observer.observe(el);
    return () => observer.disconnect();
  }, [map]);

  const toggle = (path: string) =>
    setSelected(prev => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  const cells = useMemo<Cell[]>(() => {
    if (!map || width <= 0) return [];
    const items: { entry: MapEntry | null; label: string; size: number }[] = map.entries.map(entry => ({
      entry, label: entry.name, size: entry.size,
    }));
    if (map.other > 0) items.push({ entry: null, label: t('map.other', { n: map.other_count }), size: map.other });
    items.sort((a, b) => b.size - a.size);
    const rects = squarify(items.map(i => i.size), { x: 0, y: 0, w: width, h: MAP_HEIGHT });
    return items.map((item, i) => ({
      key: item.entry?.path ?? '__other__',
      entry: item.entry,
      label: item.label,
      size: item.size,
      rect: rects[i],
    }));
  }, [map, width, t]);

  const chosen = (map?.entries ?? []).filter(e => selected.has(e.path));
  const chosenBytes = chosen.reduce((sum, e) => sum + e.size, 0);
  const canStart = chosen.length > 0 && confirmText === 'DELETE' && !preparing && !busy;
  const canGoUp = trail.length > 1;
  const share = (size: number) => (map && map.total > 0 ? (size / map.total) * 100 : 0);

  const startCleaning = async () => {
    if (!map) return;
    setPreparing(true);
    setPrepareError(null);
    try {
      const items = await invoke<CleanupItem[]>('register_map_items', {
        base: map.path,
        paths: chosen.map(e => e.path),
      });
      if (items.length > 0) onStartCleaning(items);
    } catch (err) {
      setPrepareError(t('map.prepareFailed', { error: typeof err === 'string' ? err : String(err) }));
    } finally {
      setPreparing(false);
    }
  };

  const outlineButton =
    'flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-outline text-sm text-on-surface/70 hover:text-on-surface hover:bg-surface-bright disabled:opacity-50 transition-colors';

  return (
    <div className="max-w-5xl mx-auto px-6 py-8 space-y-5">
      <header className="space-y-2">
        <h2 className="text-2xl font-semibold tracking-tight">{t('map.title')}</h2>
        <p className="text-sm text-on-surface/60">{t('map.intro')}</p>
      </header>

      <section className="bg-surface-container border border-outline rounded-2xl p-5 space-y-4">
        <div className="flex flex-wrap items-center gap-2">
          <button onClick={() => start('')} disabled={busy} className={outlineButton}>
            <Home className="w-3.5 h-3.5" /> {t('map.home')}
          </button>
          {scanPath && (
            <button onClick={() => start(scanPath)} disabled={busy} title={scanPath} className={outlineButton}>
              {t('map.scanFolder')}
            </button>
          )}
          <button onClick={chooseFolder} disabled={busy} className={outlineButton}>
            <FolderOpen className="w-3.5 h-3.5" /> {t('map.chooseFolder')}
          </button>
          {map && (
            <button onClick={refresh} disabled={busy} title={fromCache ? t('map.cached') : undefined} className={outlineButton}>
              <RefreshCw className="w-3.5 h-3.5" /> {t('map.refresh')}
            </button>
          )}
          {busy && (
            <>
              <span className="flex items-center gap-2 text-sm text-on-surface/60">
                <Loader2 className="w-4 h-4 animate-spin" /> {t('map.measuring')}
              </span>
              <button onClick={cancel} className={outlineButton}>{t('map.cancel')}</button>
            </>
          )}
        </div>

        {map && (
          <div className="flex flex-wrap items-center gap-2 text-sm" dir="ltr">
            <button
              onClick={() => goTo(trail.length - 2)}
              disabled={!canGoUp || busy}
              aria-label={t('map.up')}
              title={t('map.up')}
              className="p-1.5 rounded-md border border-outline text-on-surface/70 hover:text-on-surface hover:bg-surface-bright disabled:opacity-30"
            >
              <ArrowUp className="w-3.5 h-3.5" />
            </button>
            <nav className="flex flex-wrap items-center gap-1 font-mono text-xs min-w-0" aria-label={t('map.label', { path: tildePath(map.path) })}>
              {trail.map((p, i) => (
                <span key={p} className="flex items-center gap-1">
                  {i > 0 && <span className="text-on-surface/30">/</span>}
                  <button
                    onClick={() => goTo(i)}
                    disabled={i === trail.length - 1 || busy}
                    className={i === trail.length - 1 ? 'font-semibold' : 'text-primary hover:underline'}
                    title={p}
                  >
                    {i === 0 ? tildePath(p) : lastSegment(p)}
                  </button>
                </span>
              ))}
            </nav>
            <span className="ms-auto text-on-surface/60 tabular-nums" dir="auto">{t('map.total', { size: formatBytes(map.total) })}</span>
          </div>
        )}

        {error && <p className="text-sm text-error">{t('map.failed', { error })}</p>}
        {prepareError && <p className="text-sm text-error">{prepareError}</p>}

        {!map && !busy && !error && (
          <button
            onClick={() => start(scanPath)}
            className="px-4 py-2 bg-on-surface text-surface rounded-lg font-medium text-sm hover:opacity-90 active:scale-[0.98] transition"
          >
            {t('map.show')}
          </button>
        )}

        {map && (
          <>
            <div
              ref={boxRef}
              dir="ltr"
              className={`relative w-full overflow-hidden rounded-xl border border-outline bg-surface-dim transition-opacity ${busy ? 'opacity-50' : ''}`}
              style={{ height: MAP_HEIGHT }}
              role="group"
              aria-label={t('map.label', { path: tildePath(map.path) })}
            >
              {cells.length === 0 && (
                <p className="absolute inset-0 grid place-items-center text-sm text-on-surface/50" dir="auto">{t('map.empty')}</p>
              )}
              {cells.map(cell => {
                const { x, y, w, h } = cell.rect;
                const entry = cell.entry;
                const isSelected = entry !== null && selected.has(entry.path);
                const interactive = entry !== null && !busy;
                const roomy = w >= MIN_LABEL_W && h >= MIN_LABEL_H;
                const pct = share(cell.size);
                // Folders are tinted by how much of the folder they take; files stay neutral.
                const tint = entry?.is_dir
                  ? `color-mix(in srgb, var(--color-primary) ${Math.round(14 + Math.min(pct, 40) * 0.9)}%, var(--color-surface-container))`
                  : undefined;
                const tone = entry === null
                  ? 'border-dashed border-outline bg-surface-container text-on-surface/55'
                  : entry.is_dir
                    ? 'border-primary/30 text-on-surface hover:brightness-125'
                    : 'border-outline bg-surface-bright text-on-surface/85 hover:brightness-125';
                const title = `${cell.label} · ${formatBytes(cell.size)} · ${t('map.percent', { value: pct.toFixed(1) })}`;
                return (
                  <div
                    key={cell.key}
                    role={interactive ? 'button' : undefined}
                    tabIndex={interactive ? 0 : undefined}
                    title={title}
                    aria-label={title}
                    aria-pressed={interactive ? isSelected : undefined}
                    onClick={(e) => {
                      if (!entry || busy) return;
                      if (e.metaKey || e.ctrlKey || !entry.is_dir) toggle(entry.path);
                      else open(entry.path);
                    }}
                    onKeyDown={(e) => {
                      if (!entry || busy) return;
                      if (e.key === 'Enter' && entry.is_dir) open(entry.path);
                      if (e.key === ' ') { e.preventDefault(); toggle(entry.path); }
                    }}
                    className={`absolute box-border border p-1.5 overflow-hidden rounded-sm transition-[filter,box-shadow] select-none
                      ${tone} ${interactive ? 'cursor-pointer' : 'cursor-default'}
                      ${isSelected ? 'ring-2 ring-inset ring-primary z-10' : ''}`}
                    style={{ left: x, top: y, width: Math.max(w - 2, 0), height: Math.max(h - 2, 0), margin: 1, backgroundColor: tint }}
                  >
                    {isSelected && (
                      <span className="absolute top-1 end-1 grid place-items-center w-4 h-4 rounded-full bg-primary text-on-primary" aria-hidden>
                        <Check className="w-3 h-3" strokeWidth={3} />
                      </span>
                    )}
                    {roomy && (
                      <span className="block leading-tight pe-4">
                        <span className="flex items-center gap-1 text-xs font-medium">
                          {entry?.is_dir && <Folder className="w-3 h-3 shrink-0 opacity-70" />}
                          <span className="truncate">{cell.label}</span>
                        </span>
                        <span className="block text-[11px] opacity-70 tabular-nums">
                          {formatBytes(cell.size)}{w >= 120 ? ` · ${t('map.percent', { value: pct.toFixed(0) })}` : ''}
                        </span>
                      </span>
                    )}
                  </div>
                );
              })}
            </div>

            <p className="text-xs text-on-surface/50">{t('map.hint')}</p>

            {map.entries.length > 0 && (
              <div className="rounded-xl border border-outline overflow-hidden">
                <div className="px-4 py-2.5 border-b border-outline bg-surface-bright/50 text-xs text-on-surface/60">{t('map.listTitle')}</div>
                <ul className="divide-y divide-outline/60 max-h-[360px] overflow-y-auto">
                  {map.entries.map(entry => {
                    const isSelected = selected.has(entry.path);
                    const pct = share(entry.size);
                    return (
                      <li
                        key={entry.path}
                        className={`flex items-center gap-3 px-4 py-2 transition-colors ${isSelected ? 'bg-surface-bright' : 'hover:bg-surface-bright/50'}`}
                      >
                        <input
                          type="checkbox"
                          checked={isSelected}
                          onChange={() => toggle(entry.path)}
                          aria-label={t('map.select', { name: entry.name })}
                          className="w-4 h-4 accent-[var(--color-primary)] shrink-0"
                        />
                        {entry.is_dir
                          ? <Folder className="w-4 h-4 shrink-0 text-primary/80" />
                          : <FileIcon className="w-4 h-4 shrink-0 text-on-surface/40" />}
                        <div className="min-w-0 flex-1 space-y-1">
                          {entry.is_dir ? (
                            <button
                              onClick={() => open(entry.path)}
                              disabled={busy}
                              title={t('map.openFolder')}
                              className="block max-w-full text-sm font-medium truncate text-start hover:underline" dir="ltr"
                            >
                              {entry.name}
                            </button>
                          ) : (
                            <span className="block text-sm truncate" dir="ltr" title={entry.path}>{entry.name}</span>
                          )}
                          <div className="h-1 rounded-full bg-surface-bright overflow-hidden" aria-hidden>
                            <div className="h-full rounded-full bg-primary/70" style={{ width: `${Math.max(pct, 0.5)}%` }} />
                          </div>
                        </div>
                        <span className="w-14 text-end text-xs text-on-surface/50 tabular-nums shrink-0">{t('map.percent', { value: pct.toFixed(1) })}</span>
                        <span className="w-20 text-end text-sm tabular-nums shrink-0">{formatBytes(entry.size)}</span>
                      </li>
                    );
                  })}
                </ul>
              </div>
            )}
          </>
        )}
      </section>

      {chosen.length > 0 && (
        <section className="sticky bottom-4 bg-surface-container border border-outline rounded-2xl px-5 py-4 flex flex-wrap items-center justify-between gap-4 shadow-lg">
          <div className="text-sm tabular-nums">
            <span className="font-semibold">{tn('map.selected', chosen.length)}</span>
            <span className="text-on-surface/60">{t('map.selectedNote', { size: formatBytes(chosenBytes) })}</span>
            <button onClick={() => setSelected(new Set())} className="ms-3 text-xs underline text-on-surface/60 hover:text-on-surface">
              {t('map.clearSelection')}
            </button>
          </div>
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-error/40 bg-error/10 text-xs">
              <ShieldAlert className="w-4 h-4 text-error shrink-0" />
              <span className="text-error font-medium hidden sm:inline">{t('map.typeDelete')}</span>
              <input
                value={confirmText}
                onChange={(e) => setConfirmText(e.target.value)}
                placeholder="DELETE"
                dir="ltr"
                aria-label={t('map.typeDeleteLabel')}
                className="w-28 bg-surface border border-error/40 rounded-md px-2 py-1 text-xs font-mono outline-none focus:border-error text-error placeholder:text-error/40"
              />
            </div>
            <button
              onClick={startCleaning}
              disabled={!canStart}
              className="px-5 py-2.5 bg-on-surface text-surface rounded-lg font-medium text-sm hover:opacity-90 active:scale-[0.98] transition disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {t('map.moveToTrash')}
            </button>
          </div>
        </section>
      )}
    </div>
  );
}
