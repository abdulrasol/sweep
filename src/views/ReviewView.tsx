import { useState, useEffect, useMemo, type ReactNode } from 'react';
import { Channel } from '@tauri-apps/api/core';
import {
  Search, Check, AlertTriangle, RefreshCw, EyeOff, Lock, FolderSearch,
  ChevronDown, ShieldCheck, ShieldAlert, ArrowDownWideNarrow, X, Square,
} from 'lucide-react';
import { invoke, formatBytes } from '../lib/tauri';
import { BrandTile, groupIcon } from '../lib/brands';
import { goesPermanent } from '../lib/deleteMode';
import { moduleTitle } from '../lib/modules';
import { formatAge } from '../lib/age';
import { useI18n, type MessageKey } from '../i18n';

interface ReviewViewProps {
  scanPath: string;
  selectedModules: string[];
  ignoredPaths: string[];
  maxDepth: number;
  deleteMode: 'trash' | 'permanent';
  onIgnore: (path: string) => void;
  onStartCleaning: (items: CleanupItem[]) => void;
  onChangeScope: () => void;
}

type SafetyFilter = 'ALL' | SafetyLevel;
type SortKey = 'size' | 'name' | 'age';

type ScanProgressEvent =
  | { kind: 'phase'; label: string }
  | { kind: 'path'; path: string }
  | { kind: 'measuring'; done: number; total: number; bytes: number };

const GROUP_ORDER = ['Projects', 'Dev Tools', 'AI Tools', 'Editors', 'System & Apps'];

const SAFETY_STYLE: Record<SafetyLevel, { badge: string; dot: string; label: MessageKey }> = {
  SAFE: { badge: 'bg-safe/10 text-safe border-safe/30', dot: 'bg-safe', label: 'safety.safe' },
  REVIEW: { badge: 'bg-warning/10 text-warning border-warning/30', dot: 'bg-warning', label: 'safety.review' },
  DANGER: { badge: 'bg-error/10 text-error border-error/30', dot: 'bg-error', label: 'safety.danger' },
};

const sum = (list: CleanupItem[]) => list.reduce((acc, i) => acc + i.size_bytes, 0);

/** Full path abbreviated with '~' for home and middle ellipsis when too long. */
const formatFullPath = (path: string, max = 70) => {
  const home = path.replace(/^\/Users\/[^/]+/, '~');
  if (home.length <= max) return home;
  const keep = Math.floor((max - 1) / 2);
  return `${home.slice(0, keep)}…${home.slice(-keep)}`;
};

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');

/** Displays previous path segment (muted) and last segment (semibold). */
function renderPathTitle(path: string) {
  const parts = path.split('/').filter(Boolean);
  const last = parts[parts.length - 1] || path;
  const prev = parts.length > 1 ? parts[parts.length - 2] : null;

  return (
    <span className="text-sm font-semibold truncate" dir="ltr" title={path}>
      {prev && <span className="text-on-surface/50 font-normal">{prev} / </span>}
      <span>{last}</span>
    </span>
  );
}

/** Relative size bar width: sqrt(size / maxSize) * 100%, min 3% for size > 0. */
const sizeBarWidth = (size: number, max: number): number => {
  if (size <= 0 || max <= 0) return 0;
  const pct = Math.sqrt(size / max) * 100;
  return Math.max(3, Math.min(100, pct));
};

function Chip({ active, onClick, children, title }: {
  active: boolean; onClick: () => void; children: ReactNode; title?: string;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className={`flex items-center gap-2 px-3 py-1.5 rounded-lg border text-xs font-medium whitespace-nowrap transition-colors
        ${active
          ? 'bg-on-surface text-surface border-on-surface'
          : 'bg-surface border-outline text-on-surface/70 hover:border-outline-variant hover:text-on-surface'}`}
    >
      {children}
    </button>
  );
}

function Checkbox({ checked, partial, disabled, onClick, label }: {
  checked: boolean; partial?: boolean; disabled?: boolean; onClick: () => void; label: string;
}) {
  return (
    <button
      role="checkbox"
      aria-checked={partial ? 'mixed' : checked}
      aria-label={label}
      disabled={disabled}
      onClick={(e) => { e.stopPropagation(); onClick(); }}
      className={`w-4 h-4 shrink-0 rounded border flex items-center justify-center transition-colors
        ${checked || partial ? 'bg-on-surface border-on-surface text-surface' : 'border-outline-variant hover:border-outline text-on-surface bg-surface'}
        ${disabled ? 'opacity-30 cursor-not-allowed' : 'cursor-pointer'}`}
    >
      {checked && <Check className="w-3 h-3" strokeWidth={3} />}
      {!checked && partial && <span className="w-2 h-0.5 bg-surface rounded" />}
    </button>
  );
}

export default function ReviewView({ scanPath, selectedModules, ignoredPaths, maxDepth, deleteMode, onIgnore, onStartCleaning, onChangeScope }: ReviewViewProps) {
  const { t, td, tn } = useI18n();
  const [items, setItems] = useState<CleanupItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [scanError, setScanError] = useState<string | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [confirmText, setConfirmText] = useState('');
  const [scanNonce, setScanNonce] = useState(0);

  // Null until the engine reports a phase; the first phase is shown in the interface language.
  const [phaseLabel, setPhaseLabel] = useState<string | null>(null);
  const [currentPath, setCurrentPath] = useState('');
  const [measuring, setMeasuring] = useState<{ done: number; total: number; bytes: number } | null>(null);
  const [stopping, setStopping] = useState(false);

  const [query, setQuery] = useState('');
  const [groupFilter, setGroupFilter] = useState<string>('ALL');
  const [typeFilter, setTypeFilter] = useState<string>('ALL');
  const [safetyFilter, setSafetyFilter] = useState<SafetyFilter>('ALL');
  const [toolFilter, setToolFilter] = useState<string>('ALL');
  const [ageFilter, setAgeFilter] = useState<string>('ALL');
  const [sortKey, setSortKey] = useState<SortKey>('size');
  const [showAllTypes, setShowAllTypes] = useState(false);

  useEffect(() => {
    const run = async () => {
      setLoading(true);
      setStopping(false);
      setPhaseLabel(null);
      setCurrentPath('');
      setMeasuring(null);
      try {
        const channel = new Channel<ScanProgressEvent>();
        channel.onmessage = (event) => {
          if (event.kind === 'phase') setPhaseLabel(event.label);
          else if (event.kind === 'path') setCurrentPath(event.path);
          else if (event.kind === 'measuring') setMeasuring(event);
        };
        const data = await invoke<CleanupItem[]>('scan_environment', {
          path: scanPath,
          modules: selectedModules,
          ignoredPaths,
          maxDepth,
          onEvent: channel,
        });
        setItems(data);
        // Pre-select only SAFE items that are not blocked by a running app.
        setSelectedIds(new Set(data.filter(i => i.status === 'SAFE' && !i.blocked_by).map(i => i.id)));
        setScanError(null);
      } catch (err) {
        if (String(err) === 'Scan cancelled.') {
          onChangeScope();
          return;
        }
        console.error('Scan failed:', err);
        setScanError(String(err));
      } finally {
        setLoading(false);
      }
    };
    run();
    // ignoredPaths is read at scan time; ignoring an item removes it locally without a rescan.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scanPath, selectedModules, scanNonce, maxDepth]);

  // ----- Derived data -------------------------------------------------------

  const groups = useMemo(() => {
    const map = new Map<string, CleanupItem[]>();
    for (const i of items) map.set(i.group, [...(map.get(i.group) ?? []), i]);
    return [...map.entries()].sort(
      (a, b) => (GROUP_ORDER.indexOf(a[0]) + 99) % 99 - (GROUP_ORDER.indexOf(b[0]) + 99) % 99,
    );
  }, [items]);

  const toolOptions = useMemo(() => {
    const map = new Map<string, CleanupItem[]>();
    for (const i of items) map.set(i.module, [...(map.get(i.module) ?? []), i]);
    return [...map.entries()]
      .map(([id, list]) => ({ id, count: list.length, size: sum(list) }))
      .sort((a, b) => b.size - a.size);
  }, [items]);

  // A rescan or removals may leave the chosen tool with no items.
  useEffect(() => {
    if (toolFilter !== 'ALL' && !items.some(i => i.module === toolFilter)) setToolFilter('ALL');
  }, [items, toolFilter]);

  const inGroup = useMemo(
    () => (groupFilter === 'ALL' ? items : items.filter(i => i.group === groupFilter)),
    [items, groupFilter],
  );

  const types = useMemo(() => {
    const map = new Map<string, CleanupItem[]>();
    for (const i of inGroup) map.set(i.type, [...(map.get(i.type) ?? []), i]);
    return [...map.entries()].sort((a, b) => sum(b[1]) - sum(a[1]));
  }, [inGroup]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const nowSec = Math.floor(Date.now() / 1000);
    const olderThanSecs = ageFilter === 'ALL' ? null : Number(ageFilter) * 86_400;
    const list = inGroup.filter(i =>
      (toolFilter === 'ALL' || i.module === toolFilter) &&
      (typeFilter === 'ALL' || i.type === typeFilter) &&
      (safetyFilter === 'ALL' || i.status === safetyFilter) &&
      (olderThanSecs === null || (i.last_active !== null && nowSec - i.last_active > olderThanSecs)) &&
      (!q || i.path.toLowerCase().includes(q) || i.type.toLowerCase().includes(q) || i.file_type.toLowerCase().includes(q) || moduleTitle(i.module).toLowerCase().includes(q)),
    );
    return list.sort((a, b) => {
      if (sortKey === 'size') return b.size_bytes - a.size_bytes;
      if (sortKey === 'age') {
        return (a.last_active ?? Number.MAX_SAFE_INTEGER) - (b.last_active ?? Number.MAX_SAFE_INTEGER);
      }
      return a.path.localeCompare(b.path);
    });
  }, [inGroup, toolFilter, typeFilter, safetyFilter, ageFilter, query, sortKey]);

  const sections = useMemo(() => {
    if (groupFilter !== 'ALL') return [[groupFilter, visible] as [string, CleanupItem[]]];
    const map = new Map<string, CleanupItem[]>();
    for (const i of visible) map.set(i.group, [...(map.get(i.group) ?? []), i]);
    return [...map.entries()].sort((a, b) => sum(b[1]) - sum(a[1]));
  }, [visible, groupFilter]);

  const maxSize = visible.reduce((m, i) => Math.max(m, i.size_bytes), 1);
  const selectedItems = items.filter(i => selectedIds.has(i.id));
  const selectedBytes = sum(selectedItems);
  const permanentCount = deleteMode === 'trash' ? selectedItems.filter(goesPermanent).length : 0;
  const needsConfirm = selectedItems.some(i => i.status === 'DANGER');
  const canStart = selectedItems.length > 0 && (!needsConfirm || confirmText === 'DELETE');
  const safetyCounts = (['SAFE', 'REVIEW', 'DANGER'] as SafetyLevel[]).map(
    s => [s, items.filter(i => i.status === s)] as const,
  );
  const filtersActive = groupFilter !== 'ALL' || typeFilter !== 'ALL' || safetyFilter !== 'ALL' || toolFilter !== 'ALL' || ageFilter !== 'ALL' || query !== '';

  // ----- Actions -------------------------------------------------------------

  const setSelection = (ids: string[], on: boolean) => {
    setSelectedIds(prev => {
      const next = new Set(prev);
      ids.forEach(id => (on ? next.add(id) : next.delete(id)));
      return next;
    });
  };

  const toggleItem = (item: CleanupItem) => {
    if (item.blocked_by) return;
    setSelection([item.id], !selectedIds.has(item.id));
  };

  const selectionState = (list: CleanupItem[]) => {
    const selectable = list.filter(i => !i.blocked_by);
    const n = selectable.filter(i => selectedIds.has(i.id)).length;
    return { selectable, all: n > 0 && n === selectable.length, some: n > 0 && n < selectable.length };
  };

  const toggleList = (list: CleanupItem[]) => {
    const { selectable, all } = selectionState(list);
    setSelection(selectable.map(i => i.id), !all);
  };

  const handleIgnore = (item: CleanupItem) => {
    onIgnore(item.path);
    setItems(prev => prev.filter(i => i.id !== item.id));
    setSelection([item.id], false);
  };

  const reveal = (item: CleanupItem) => {
    invoke('reveal_item', { id: item.id }).catch(err => console.error(err));
  };

  const clearFilters = () => {
    setGroupFilter('ALL');
    setTypeFilter('ALL');
    setSafetyFilter('ALL');
    setToolFilter('ALL');
    setAgeFilter('ALL');
    setQuery('');
  };

  const stopScan = () => {
    setStopping(true);
    invoke('cancel_scan').catch(err => console.error(err));
  };

  // ----- Render ---------------------------------------------------------------

  if (loading) {
    const progress = measuring && measuring.total > 0
      ? Math.max((measuring.done / measuring.total) * 100, 1.5)
      : 0;
    return (
      <div className="flex flex-col items-center justify-center h-full space-y-4">
        <RefreshCw className="w-8 h-8 text-primary animate-spin opacity-60" />
        <p className="text-sm text-on-surface/60">{phaseLabel ?? t('review.phaseInit')}…</p>
        <p className="text-xs font-mono text-on-surface/40 max-w-md truncate" title={currentPath || scanPath}>
          {tildePath(currentPath || scanPath)}
        </p>
        {measuring && (
          <div className="w-full max-w-md space-y-1">
            <div
              className="h-2 rounded-full bg-surface-bright overflow-hidden"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={measuring.total}
              aria-valuenow={measuring.done}
            >
              <div
                className="h-full rounded-full bg-primary transition-[width] duration-300"
                style={{ width: `${progress}%` }}
              />
            </div>
            <div className="flex justify-between text-xs text-on-surface/55 tabular-nums">
              <span>{t('review.progress', { done: measuring.done, total: measuring.total })}</span>
              <span>{formatBytes(measuring.bytes)}</span>
            </div>
          </div>
        )}
        <button
          onClick={stopScan}
          disabled={stopping}
          className="flex items-center gap-2 px-4 py-2 rounded-lg border border-outline text-sm font-medium hover:bg-surface-bright transition-colors disabled:opacity-50"
        >
          <Square className="w-3.5 h-3.5" />
          {stopping ? t('review.stopping') : t('review.stop')}
        </button>
      </div>
    );
  }

  const allVisible = selectionState(visible);
  const nowMs = Date.now();

  return (
    <div className="min-h-full flex flex-col justify-between">
      <div className="max-w-6xl w-full mx-auto py-6 px-6 space-y-4 flex-1 pb-16">
        {/* Summary */}
        <section className="bg-surface-container border border-outline rounded-2xl p-6 flex flex-wrap items-center justify-between gap-6">
          <div className="flex-1 min-w-[240px] space-y-1">
            <h2 className="text-xl font-semibold tracking-tight">{t('review.title')}</h2>
            <p className="text-sm text-on-surface/60">
              {tn('review.summary', items.length, { size: formatBytes(sum(items)) })}
            </p>
            <div className="flex flex-wrap gap-3 pt-2 text-xs">
              {safetyCounts.map(([s, list]) => (
                <span key={s} className="flex items-center gap-1.5 text-on-surface/70">
                  <span className={`w-2 h-2 rounded-full ${SAFETY_STYLE[s].dot}`} />
                  {t(SAFETY_STYLE[s].label)} {list.length} · {formatBytes(sum(list))}
                </span>
              ))}
            </div>
          </div>

          <div>
            <button
              onClick={() => setScanNonce(n => n + 1)}
              className="flex items-center justify-center gap-2 px-5 py-2.5 border border-outline rounded-lg text-xs font-medium text-on-surface/70 hover:bg-surface-bright transition"
            >
              <RefreshCw className="w-3.5 h-3.5" /> {t('review.scanAgain')}
            </button>
          </div>

          {scanError && (
            <p className="basis-full text-sm text-error">
              {t('review.scanFailed', { error: scanError })}{' '}
              <button onClick={onChangeScope} className="underline">{t('review.changeScope')}</button>
            </p>
          )}
        </section>

        {/* Groups and Types (non-sticky, scrolls with page) */}
        <div className="space-y-3">
          <div className="flex flex-wrap gap-2">
            <Chip active={groupFilter === 'ALL'} onClick={() => { setGroupFilter('ALL'); setTypeFilter('ALL'); }}>
              {t('review.all')} <span className="opacity-60 tabular-nums">{formatBytes(sum(items))}</span>
            </Chip>
            {groups.map(([g, list]) => (
              <Chip key={g} active={groupFilter === g} onClick={() => { setGroupFilter(g); setTypeFilter('ALL'); }}>
                {td(`group.${g}`, g)} <span className="opacity-60 tabular-nums">{list.length} · {formatBytes(sum(list))}</span>
              </Chip>
            ))}
          </div>

          {types.length > 1 && (
            <div className="flex flex-wrap items-center gap-1.5">
              <button
                onClick={() => setTypeFilter('ALL')}
                className={`px-2.5 py-1 rounded-md text-[11px] font-medium whitespace-nowrap border transition-colors
                  ${typeFilter === 'ALL' ? 'border-primary/50 bg-primary/10 text-primary' : 'border-transparent text-on-surface/60 hover:text-on-surface'}`}
              >
                {t('review.allTypes')}
              </button>
              {(showAllTypes ? types : types.slice(0, 8)).map(([t, list]) => (
                <button
                  key={t}
                  onClick={() => setTypeFilter(t)}
                  className={`px-2.5 py-1 rounded-md text-[11px] font-medium whitespace-nowrap border transition-colors
                    ${typeFilter === t ? 'border-primary/50 bg-primary/10 text-primary' : 'border-transparent text-on-surface/60 hover:text-on-surface'}`}
                >
                  {t} <span className="opacity-60 tabular-nums">{formatBytes(sum(list))}</span>
                </button>
              ))}
              {types.length > 8 && (
                <button
                  onClick={() => setShowAllTypes(prev => !prev)}
                  className="px-2.5 py-1 rounded-md text-[11px] font-medium text-on-surface/60 hover:text-on-surface hover:bg-surface-bright border border-outline/50 transition-colors"
                >
                  {showAllTypes ? t('review.showLess') : t('review.moreTypes', { n: types.length - 8 })}
                </button>
              )}
            </div>
          )}
        </div>

        {/* Sticky filters: single compact row */}
        <section className="sticky top-0 z-10 bg-surface-dim/95 backdrop-blur border border-outline rounded-2xl p-3">
          <div className="flex flex-wrap items-center gap-3">
            <div className="flex rounded-lg border border-outline overflow-hidden text-xs shrink-0" role="group" aria-label={t('review.safetyLevel')}>
              {(['ALL', 'SAFE', 'REVIEW', 'DANGER'] as SafetyFilter[]).map(s => (
                <button
                  key={s}
                  onClick={() => setSafetyFilter(s)}
                  className={`px-3 py-1.5 font-medium transition-colors flex items-center gap-1.5
                    ${safetyFilter === s ? 'bg-on-surface text-surface' : 'bg-surface text-on-surface/70 hover:text-on-surface'}`}
                >
                  {s !== 'ALL' && <span className={`w-1.5 h-1.5 rounded-full ${SAFETY_STYLE[s].dot}`} />}
                  {s === 'ALL' ? t('review.anyLevel') : t(SAFETY_STYLE[s].label)}
                </button>
              ))}
            </div>

            <div className="relative flex-1 min-w-[180px]">
              <Search className="absolute start-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-on-surface/40" />
              <input
                id="review-search"
                placeholder={t('review.searchPlaceholder')}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                className="w-full bg-surface border border-outline rounded-lg ps-9 pe-3 py-1.5 text-xs outline-none focus:border-primary/60 placeholder:text-on-surface/40"
              />
            </div>

            {toolOptions.length > 1 && (
              <select
                aria-label={t('review.tool')}
                value={toolFilter}
                onChange={(e) => setToolFilter(e.target.value)}
                className="bg-surface border border-outline rounded-lg px-3 py-1.5 text-xs text-on-surface/70 outline-none focus:border-primary/60 shrink-0"
              >
                <option value="ALL">{t('review.allTools')}</option>
                {toolOptions.map(tool => (
                  <option key={tool.id} value={tool.id}>
                    {td(`module.${tool.id}.title`, moduleTitle(tool.id))} · {tool.count} · {formatBytes(tool.size)}
                  </option>
                ))}
              </select>
            )}

            <select
              aria-label={t('review.activity')}
              value={ageFilter}
              onChange={(e) => setAgeFilter(e.target.value)}
              className="bg-surface border border-outline rounded-lg px-3 py-1.5 text-xs text-on-surface/70 outline-none focus:border-primary/60 shrink-0"
            >
              <option value="ALL">{t('review.anyActivity')}</option>
              <option value="30">{t('review.olderDays', { n: 30 })}</option>
              <option value="90">{t('review.olderDays', { n: 90 })}</option>
              <option value="180">{t('review.olderDays', { n: 180 })}</option>
              <option value="365">{t('review.olderYear')}</option>
            </select>

            <button
              onClick={() => setSortKey(k => (k === 'size' ? 'name' : k === 'name' ? 'age' : 'size'))}
              className="flex items-center gap-1.5 px-3 py-1.5 border border-outline rounded-lg text-xs text-on-surface/70 hover:text-on-surface bg-surface shrink-0"
            >
              <ArrowDownWideNarrow className="w-3.5 h-3.5" />
              {sortKey === 'size' ? t('review.sortSize') : sortKey === 'name' ? t('review.sortName') : t('review.sortAge')}
            </button>

            {filtersActive && (
              <button onClick={clearFilters} className="flex items-center gap-1 text-xs text-on-surface/60 hover:text-on-surface shrink-0">
                <X className="w-3.5 h-3.5" /> {t('review.clearFilters')}
              </button>
            )}
          </div>
        </section>

        {/* List */}
        <section className="bg-surface-container border border-outline rounded-2xl overflow-hidden">
          <div className="flex items-center gap-3 px-4 py-3 border-b border-outline text-xs text-on-surface/70">
            <Checkbox
              checked={allVisible.all}
              partial={allVisible.some}
              disabled={allVisible.selectable.length === 0}
              onClick={() => toggleList(visible)}
              label={t('review.selectAllShown')}
            />
            <span className="font-medium">
              {t('review.shown', { n: visible.length, size: formatBytes(sum(visible)) })}
            </span>
            <button
              onClick={() => setSelection(visible.filter(i => i.status === 'SAFE' && !i.blocked_by).map(i => i.id), true)}
              className="ms-auto flex items-center gap-1.5 text-primary hover:underline"
            >
              <ShieldCheck className="w-3.5 h-3.5" /> {t('review.selectSafeShown')}
            </button>
            <button onClick={() => setSelection(visible.map(i => i.id), false)} className="hover:text-on-surface">
              {t('review.clearSelection')}
            </button>
          </div>

          {visible.length === 0 && (
            <div className="p-10 text-center space-y-3">
              <p className="text-sm text-on-surface/60">
                {items.length === 0 ? t('review.nothingToClean') : t('review.noMatch')}
              </p>
              {items.length === 0 && !scanError && (
                <button onClick={onChangeScope} className="text-xs text-primary hover:underline">{t('review.changeFolder')}</button>
              )}
              {filtersActive && (
                <button onClick={clearFilters} className="text-xs text-primary hover:underline">{t('review.clearFilters')}</button>
              )}
            </div>
          )}

          {sections.map(([group, list]) => {
            if (list.length === 0) return null;
            const sel = selectionState(list);
            const GroupIcon = groupIcon(group);
            return (
              <div key={group}>
                {groupFilter === 'ALL' && (
                  <div className="flex items-center gap-3 px-4 py-2 bg-surface-bright/60 border-b border-outline text-xs font-semibold text-on-surface/70">
                    <Checkbox
                      checked={sel.all}
                      partial={sel.some}
                      disabled={sel.selectable.length === 0}
                      onClick={() => toggleList(list)}
                      label={t('review.selectAllIn', { group: td(`group.${group}`, group) })}
                    />
                    <span className="flex items-center gap-2">
                      <GroupIcon className="w-4 h-4 text-on-surface/50" />
                      <span>{td(`group.${group}`, group)}</span>
                    </span>
                    <span className="ms-auto font-normal font-mono tabular-nums">{list.length} · {formatBytes(sum(list))}</span>
                  </div>
                )}

                <ul className="divide-y divide-outline/60">
                  {list.map(item => {
                    const selected = selectedIds.has(item.id);
                    const open = expanded.has(item.id);
                    const style = SAFETY_STYLE[item.status];
                    return (
                      <li
                        key={item.id}
                        onClick={() => toggleItem(item)}
                        className={`grid grid-cols-[auto_auto_minmax(0,1fr)_auto] gap-x-4 px-4 py-3 border-s-2 transition-colors
                          ${selected ? 'border-s-on-surface/40 bg-surface-bright' : 'border-s-transparent hover:bg-surface-bright/50'}
                          ${item.blocked_by ? 'cursor-not-allowed' : 'cursor-pointer'}`}
                      >
                        <div className="pt-0.5">
                          <Checkbox
                            checked={selected}
                            disabled={!!item.blocked_by}
                            onClick={() => toggleItem(item)}
                            label={t('review.select', { path: item.path })}
                          />
                        </div>

                        <div className="pt-0.5">
                          <BrandTile item={item} />
                        </div>

                        <div className="min-w-0 space-y-1">
                          <div className="flex items-center gap-2 min-w-0">
                            {item.status === 'DANGER' && <AlertTriangle className="w-3.5 h-3.5 text-error shrink-0" />}
                            {renderPathTitle(item.path)}
                            <span className="text-[11px] px-1.5 py-0.5 rounded bg-surface-bright text-on-surface/70 whitespace-nowrap">{item.type}</span>
                            {item.file_type && item.file_type !== item.type && (
                              <span className="text-[11px] text-on-surface/50 whitespace-nowrap truncate">{item.file_type}</span>
                            )}
                          </div>
                          <div className="text-[11px] font-mono text-on-surface/50 truncate" dir="ltr" title={item.path}>
                            {formatFullPath(item.path)}
                          </div>
                          <p className={`text-xs text-on-surface/60 leading-relaxed ${open ? '' : 'line-clamp-2'}`}>
                            {item.description}
                          </p>
                          {item.last_active !== null && (
                            <p className="text-[11px] text-on-surface/45">
                              {t('review.lastActive', { age: formatAge(item.last_active, nowMs, { t, tn }) })}
                            </p>
                          )}
                          {item.blocked_by && (
                            <p className="flex items-center gap-1.5 text-xs font-medium text-error">
                              <Lock className="w-3 h-3" /> {t('review.closeToRemove', { app: item.blocked_by })}
                            </p>
                          )}
                        </div>

                        <div className="flex flex-col items-end gap-1.5 min-w-[150px]">
                          <div className="flex items-center gap-2">
                            <span className="text-sm font-mono font-medium tabular-nums whitespace-nowrap">{formatBytes(item.size_bytes)}</span>
                            <span className={`text-[11px] font-medium px-1.5 py-0.5 rounded border ${style.badge}`}>
                              {t(style.label)}
                            </span>
                          </div>
                          <div className="w-full h-1 rounded-full bg-surface-bright overflow-hidden" aria-hidden>
                            <div className={`h-full ${style.dot} opacity-70`} style={{ width: `${sizeBarWidth(item.size_bytes, maxSize)}%` }} />
                          </div>
                          <div className="flex items-center gap-0.5 text-on-surface/50" onClick={e => e.stopPropagation()}>
                            {!item.path.includes('://') && (
                              <button onClick={() => reveal(item)} title={t('review.showInFinder')} className="p-1.5 rounded-md hover:bg-surface-bright hover:text-on-surface">
                                <FolderSearch className="w-3.5 h-3.5" />
                              </button>
                            )}
                            <button onClick={() => handleIgnore(item)} title={t('review.neverShow')} className="p-1.5 rounded-md hover:bg-surface-bright hover:text-error">
                              <EyeOff className="w-3.5 h-3.5" />
                            </button>
                            <button
                              onClick={() => setExpanded(prev => { const n = new Set(prev); if (n.has(item.id)) n.delete(item.id); else n.add(item.id); return n; })}
                              title={open ? t('review.lessDetail') : t('review.moreDetail')}
                              aria-expanded={open}
                              className="p-1.5 rounded-md hover:bg-surface-bright hover:text-on-surface"
                            >
                              <ChevronDown className={`w-3.5 h-3.5 transition-transform ${open ? 'rotate-180' : ''}`} />
                            </button>
                          </div>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              </div>
            );
          })}
        </section>
      </div>

      {/* Fixed bottom action bar */}
      {selectedItems.length > 0 && (
        <aside
          aria-label={t('review.selectionActions')}
          className="sticky bottom-0 z-20 bg-surface-container border-t border-outline shadow-lg"
        >
          <div className="max-w-6xl mx-auto px-6 py-3.5 flex flex-wrap items-center justify-between gap-4">
            <div className="flex items-center gap-4">
              <div>
                <span className="text-sm font-semibold text-on-surface">
                  {tn('review.selected', selectedItems.length)}
                </span>
                <span className="mx-2 text-on-surface/40">·</span>
                <span className="text-sm font-mono text-on-surface/80 tabular-nums">
                  {formatBytes(selectedBytes)}
                </span>
              </div>
              <button
                onClick={() => setSelectedIds(new Set())}
                className="text-xs text-on-surface/60 hover:text-on-surface underline transition-colors"
              >
                {t('review.clearSelection')}
              </button>
              {permanentCount > 0 && (
                <span className="text-xs text-warning">
                  {tn('review.permanent', permanentCount)}
                </span>
              )}
            </div>

            <div className="flex items-center gap-3">
              {needsConfirm && (
                <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-error/40 bg-error/10 text-xs">
                  <ShieldAlert className="w-4 h-4 text-error shrink-0" />
                  <span className="text-error font-medium hidden sm:inline">
                    {t('review.typeDelete')}
                  </span>
                  <input
                    id="danger-confirm"
                    value={confirmText}
                    onChange={(e) => setConfirmText(e.target.value)}
                    placeholder="DELETE"
                    dir="ltr"
                    className="w-28 bg-surface border border-error/40 rounded-md px-2 py-1 text-xs font-mono outline-none focus:border-error text-error placeholder:text-error/40"
                  />
                </div>
              )}

              <button
                onClick={() => onStartCleaning(selectedItems)}
                disabled={!canStart}
                className="px-5 py-2.5 bg-on-surface text-surface rounded-lg font-medium text-sm hover:opacity-90 active:scale-[0.98] transition disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {t('review.removeSelected')}
              </button>
            </div>
          </div>
        </aside>
      )}
    </div>
  );
}
