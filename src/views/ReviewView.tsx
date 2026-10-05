import { useState, useEffect, useMemo, type ReactNode } from 'react';
import {
  Search, Check, AlertTriangle, RefreshCw, EyeOff, Lock, FolderSearch,
  ChevronDown, ShieldCheck, ShieldAlert, ArrowDownWideNarrow, X,
} from 'lucide-react';
import { invoke, formatBytes } from '../lib/tauri';

interface SystemInfo {
  os_name: string;
  os_version: string;
  cpu_usage: number;
  ram_total: number;
  ram_used: number;
  disk_total: number;
  disk_free: number;
}

interface ReviewViewProps {
  scanPath: string;
  selectedModules: string[];
  ignoredPaths: string[];
  sysInfo: SystemInfo | null;
  onIgnore: (path: string) => void;
  onStartCleaning: (items: CleanupItem[]) => void;
}

type SafetyFilter = 'ALL' | SafetyLevel;
type SortKey = 'size' | 'name';

const GROUP_ORDER = ['Projects', 'Dev Tools', 'AI Tools', 'Editors', 'System & Apps'];

const SAFETY_STYLE: Record<SafetyLevel, { badge: string; dot: string; label: string }> = {
  SAFE: { badge: 'bg-primary/10 text-primary border-primary/25', dot: 'bg-primary', label: 'Safe' },
  REVIEW: { badge: 'bg-amber-500/10 text-amber-500 border-amber-500/30', dot: 'bg-amber-500', label: 'Review' },
  DANGER: { badge: 'bg-error/10 text-error border-error/30', dot: 'bg-error', label: 'Danger' },
};

const sum = (list: CleanupItem[]) => list.reduce((acc, i) => acc + i.size_bytes, 0);

/** "…/codes/flutter/app/build" style title: the last two path segments. */
const shortTitle = (path: string) => {
  const parts = path.split('/').filter(Boolean);
  return parts.slice(-2).join(' / ');
};

/** Parent folder, shortened in the middle so both ends stay readable. */
const parentPath = (path: string, max = 70) => {
  const parent = path.split('/').slice(0, -2).join('/') || '/';
  const home = parent.replace(/^\/Users\/[^/]+/, '~');
  if (home.length <= max) return home;
  const keep = Math.floor((max - 1) / 2);
  return `${home.slice(0, keep)}…${home.slice(-keep)}`;
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
        ${checked || partial ? 'bg-primary border-primary text-on-primary' : 'border-outline-variant hover:border-primary bg-surface'}
        ${disabled ? 'opacity-30 cursor-not-allowed' : 'cursor-pointer'}`}
    >
      {checked && <Check className="w-3 h-3" strokeWidth={3} />}
      {!checked && partial && <span className="w-2 h-0.5 bg-on-primary rounded" />}
    </button>
  );
}

export default function ReviewView({ scanPath, selectedModules, ignoredPaths, onIgnore, onStartCleaning }: ReviewViewProps) {
  const [items, setItems] = useState<CleanupItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [scanError, setScanError] = useState<string | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [confirmText, setConfirmText] = useState('');
  const [scanNonce, setScanNonce] = useState(0);

  const [query, setQuery] = useState('');
  const [groupFilter, setGroupFilter] = useState<string>('ALL');
  const [typeFilter, setTypeFilter] = useState<string>('ALL');
  const [safetyFilter, setSafetyFilter] = useState<SafetyFilter>('ALL');
  const [sortKey, setSortKey] = useState<SortKey>('size');

  useEffect(() => {
    const run = async () => {
      setLoading(true);
      try {
        const data = await invoke<CleanupItem[]>('scan_environment', {
          path: scanPath,
          modules: selectedModules,
          ignoredPaths,
        });
        setItems(data);
        // Pre-select only SAFE items that are not blocked by a running app.
        setSelectedIds(new Set(data.filter(i => i.status === 'SAFE' && !i.blocked_by).map(i => i.id)));
        setScanError(null);
      } catch (err) {
        console.error('Scan failed:', err);
        setScanError(String(err));
      } finally {
        setLoading(false);
      }
    };
    run();
    // ignoredPaths is read at scan time; ignoring an item removes it locally without a rescan.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scanPath, selectedModules, scanNonce]);

  // ----- Derived data -------------------------------------------------------

  const groups = useMemo(() => {
    const map = new Map<string, CleanupItem[]>();
    for (const i of items) map.set(i.group, [...(map.get(i.group) ?? []), i]);
    return [...map.entries()].sort(
      (a, b) => (GROUP_ORDER.indexOf(a[0]) + 99) % 99 - (GROUP_ORDER.indexOf(b[0]) + 99) % 99,
    );
  }, [items]);

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
    const list = inGroup.filter(i =>
      (typeFilter === 'ALL' || i.type === typeFilter) &&
      (safetyFilter === 'ALL' || i.status === safetyFilter) &&
      (!q || i.path.toLowerCase().includes(q) || i.type.toLowerCase().includes(q) || i.file_type.toLowerCase().includes(q)),
    );
    return list.sort((a, b) => (sortKey === 'size' ? b.size_bytes - a.size_bytes : a.path.localeCompare(b.path)));
  }, [inGroup, typeFilter, safetyFilter, query, sortKey]);

  const sections = useMemo(() => {
    if (groupFilter !== 'ALL') return [[groupFilter, visible] as [string, CleanupItem[]]];
    const map = new Map<string, CleanupItem[]>();
    for (const i of visible) map.set(i.group, [...(map.get(i.group) ?? []), i]);
    return [...map.entries()].sort((a, b) => sum(b[1]) - sum(a[1]));
  }, [visible, groupFilter]);

  const maxSize = visible.reduce((m, i) => Math.max(m, i.size_bytes), 1);
  const selectedItems = items.filter(i => selectedIds.has(i.id));
  const selectedBytes = sum(selectedItems);
  const needsConfirm = selectedItems.some(i => i.status === 'DANGER');
  const canStart = selectedItems.length > 0 && (!needsConfirm || confirmText === 'DELETE');
  const safetyCounts = (['SAFE', 'REVIEW', 'DANGER'] as SafetyLevel[]).map(
    s => [s, items.filter(i => i.status === s)] as const,
  );
  const filtersActive = groupFilter !== 'ALL' || typeFilter !== 'ALL' || safetyFilter !== 'ALL' || query !== '';

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
    setQuery('');
  };

  // ----- Render ---------------------------------------------------------------

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-full space-y-4">
        <RefreshCw className="w-8 h-8 text-primary animate-spin opacity-60" />
        <p className="text-sm text-on-surface/60">Scanning {selectedModules.length} modules…</p>
        <p className="text-xs font-mono text-on-surface/40 max-w-md truncate">{scanPath}</p>
      </div>
    );
  }

  const [selValue, selUnit] = formatBytes(selectedBytes).split(' ');
  const allVisible = selectionState(visible);

  return (
    <div className="max-w-6xl mx-auto py-6 px-6 space-y-4">
      {/* Summary */}
      <section className="bg-surface-container border border-outline rounded-2xl p-6 flex flex-wrap items-center gap-6">
        <div className="flex-1 min-w-[240px] space-y-1">
          <h2 className="text-xl font-semibold tracking-tight">Review what to remove</h2>
          <p className="text-sm text-on-surface/60">
            {items.length} items, {formatBytes(sum(items))} in total.
            Safe items are selected for you; Review and Danger items are not.
          </p>
          <div className="flex flex-wrap gap-3 pt-2 text-xs">
            {safetyCounts.map(([s, list]) => (
              <span key={s} className="flex items-center gap-1.5 text-on-surface/70">
                <span className={`w-2 h-2 rounded-full ${SAFETY_STYLE[s].dot}`} />
                {SAFETY_STYLE[s].label} {list.length} · {formatBytes(sum(list))}
              </span>
            ))}
          </div>
        </div>

        <div className="text-right">
          <div className="text-[11px] uppercase tracking-wider text-on-surface/50 font-semibold">Selected</div>
          <div className="flex items-baseline justify-end gap-1.5">
            <span className="text-4xl font-light tracking-tight tabular-nums">{selValue}</span>
            <span className="text-sm text-on-surface/60 font-mono">{selUnit}</span>
          </div>
          <div className="text-xs text-on-surface/50">{selectedItems.length} items</div>
        </div>

        <div className="flex flex-col gap-2">
          <button
            onClick={() => onStartCleaning(selectedItems)}
            disabled={!canStart}
            className="px-5 py-2.5 bg-on-surface text-surface rounded-lg font-semibold text-xs uppercase tracking-wider hover:brightness-90 active:scale-[0.98] transition disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Remove selected
          </button>
          <button
            onClick={() => setScanNonce(n => n + 1)}
            className="flex items-center justify-center gap-2 px-5 py-2 border border-outline rounded-lg text-xs font-medium text-on-surface/70 hover:bg-surface-bright transition"
          >
            <RefreshCw className="w-3.5 h-3.5" /> Scan again
          </button>
        </div>

        {needsConfirm && (
          <div className="basis-full flex flex-wrap items-center gap-3 p-3 rounded-xl border border-error/30 bg-error/10">
            <ShieldAlert className="w-4 h-4 text-error shrink-0" />
            <p className="text-xs text-error font-medium flex-1 min-w-[200px]">
              Your selection includes Danger items that cannot be recovered. Type DELETE to confirm.
            </p>
            <input
              id="danger-confirm"
              value={confirmText}
              onChange={(e) => setConfirmText(e.target.value)}
              placeholder="DELETE"
              className="w-36 bg-surface border border-error/40 rounded-lg px-3 py-1.5 text-xs font-mono outline-none focus:border-error"
            />
          </div>
        )}
        {scanError && <p className="basis-full text-xs text-error font-medium">Scan failed: {scanError}</p>}
      </section>

      {/* Filters */}
      <section className="sticky top-0 z-10 bg-surface-dim/95 backdrop-blur border border-outline rounded-2xl p-4 space-y-3">
        <div className="flex gap-2 overflow-x-auto pb-1">
          <Chip active={groupFilter === 'ALL'} onClick={() => { setGroupFilter('ALL'); setTypeFilter('ALL'); }}>
            All <span className="opacity-60 tabular-nums">{formatBytes(sum(items))}</span>
          </Chip>
          {groups.map(([g, list]) => (
            <Chip key={g} active={groupFilter === g} onClick={() => { setGroupFilter(g); setTypeFilter('ALL'); }}>
              {g} <span className="opacity-60 tabular-nums">{list.length} · {formatBytes(sum(list))}</span>
            </Chip>
          ))}
        </div>

        {types.length > 1 && (
          <div className="flex gap-1.5 overflow-x-auto pb-1">
            <button
              onClick={() => setTypeFilter('ALL')}
              className={`px-2.5 py-1 rounded-md text-[11px] font-medium whitespace-nowrap border transition-colors
                ${typeFilter === 'ALL' ? 'border-primary/50 bg-primary/10 text-primary' : 'border-transparent text-on-surface/60 hover:text-on-surface'}`}
            >
              All types
            </button>
            {types.map(([t, list]) => (
              <button
                key={t}
                onClick={() => setTypeFilter(t)}
                className={`px-2.5 py-1 rounded-md text-[11px] font-medium whitespace-nowrap border transition-colors
                  ${typeFilter === t ? 'border-primary/50 bg-primary/10 text-primary' : 'border-transparent text-on-surface/60 hover:text-on-surface'}`}
              >
                {t} <span className="opacity-60 tabular-nums">{formatBytes(sum(list))}</span>
              </button>
            ))}
          </div>
        )}

        <div className="flex flex-wrap items-center gap-3">
          <div className="flex rounded-lg border border-outline overflow-hidden text-xs" role="group" aria-label="Safety level">
            {(['ALL', 'SAFE', 'REVIEW', 'DANGER'] as SafetyFilter[]).map(s => (
              <button
                key={s}
                onClick={() => setSafetyFilter(s)}
                className={`px-3 py-1.5 font-medium transition-colors flex items-center gap-1.5
                  ${safetyFilter === s ? 'bg-on-surface text-surface' : 'bg-surface text-on-surface/70 hover:text-on-surface'}`}
              >
                {s !== 'ALL' && <span className={`w-1.5 h-1.5 rounded-full ${SAFETY_STYLE[s].dot}`} />}
                {s === 'ALL' ? 'Any level' : SAFETY_STYLE[s].label}
              </button>
            ))}
          </div>

          <div className="relative flex-1 min-w-[180px]">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-on-surface/40" />
            <input
              id="review-search"
              placeholder="Search path, type or tool"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="w-full bg-surface border border-outline rounded-lg pl-9 pr-3 py-1.5 text-xs outline-none focus:border-primary/60 placeholder:text-on-surface/40"
            />
          </div>

          <button
            onClick={() => setSortKey(k => (k === 'size' ? 'name' : 'size'))}
            className="flex items-center gap-1.5 px-3 py-1.5 border border-outline rounded-lg text-xs text-on-surface/70 hover:text-on-surface bg-surface"
          >
            <ArrowDownWideNarrow className="w-3.5 h-3.5" />
            {sortKey === 'size' ? 'Largest first' : 'By path'}
          </button>

          {filtersActive && (
            <button onClick={clearFilters} className="flex items-center gap-1 text-xs text-on-surface/60 hover:text-on-surface">
              <X className="w-3.5 h-3.5" /> Clear filters
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
            label="Select all shown items"
          />
          <span className="font-medium">
            {visible.length} shown · {formatBytes(sum(visible))}
          </span>
          <button
            onClick={() => setSelection(visible.filter(i => i.status === 'SAFE' && !i.blocked_by).map(i => i.id), true)}
            className="ml-auto flex items-center gap-1.5 text-primary hover:underline"
          >
            <ShieldCheck className="w-3.5 h-3.5" /> Select safe shown
          </button>
          <button onClick={() => setSelection(visible.map(i => i.id), false)} className="hover:text-on-surface">
            Clear selection
          </button>
        </div>

        {visible.length === 0 && (
          <div className="p-10 text-center space-y-3">
            <p className="text-sm text-on-surface/60">
              {items.length === 0 ? 'Nothing to clean in the selected modules.' : 'No items match these filters.'}
            </p>
            {filtersActive && (
              <button onClick={clearFilters} className="text-xs text-primary hover:underline">Clear filters</button>
            )}
          </div>
        )}

        {sections.map(([group, list]) => {
          if (list.length === 0) return null;
          const sel = selectionState(list);
          return (
            <div key={group}>
              {groupFilter === 'ALL' && (
                <div className="flex items-center gap-3 px-4 py-2 bg-surface-bright/60 border-b border-outline text-[11px] uppercase tracking-wider font-semibold text-on-surface/60">
                  <Checkbox
                    checked={sel.all}
                    partial={sel.some}
                    disabled={sel.selectable.length === 0}
                    onClick={() => toggleList(list)}
                    label={`Select all in ${group}`}
                  />
                  <span>{group}</span>
                  <span className="ml-auto normal-case tracking-normal font-mono">{list.length} · {formatBytes(sum(list))}</span>
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
                      className={`grid grid-cols-[auto_minmax(0,1fr)_auto] gap-x-4 px-4 py-3 border-l-2 transition-colors
                        ${selected ? 'border-l-primary bg-primary/[0.04]' : 'border-l-transparent hover:bg-surface-bright/50'}
                        ${item.blocked_by ? 'cursor-not-allowed' : 'cursor-pointer'}`}
                    >
                      <div className="pt-0.5">
                        <Checkbox
                          checked={selected}
                          disabled={!!item.blocked_by}
                          onClick={() => toggleItem(item)}
                          label={`Select ${item.path}`}
                        />
                      </div>

                      <div className="min-w-0 space-y-1">
                        <div className="flex items-center gap-2 min-w-0">
                          {item.status === 'DANGER' && <AlertTriangle className="w-3.5 h-3.5 text-error shrink-0" />}
                          <span className="text-sm font-semibold truncate" title={item.path}>{shortTitle(item.path)}</span>
                          <span className="text-[11px] px-1.5 py-0.5 rounded bg-surface-bright text-on-surface/70 whitespace-nowrap">{item.type}</span>
                          {item.file_type && item.file_type !== item.type && (
                            <span className="text-[11px] text-on-surface/50 whitespace-nowrap truncate">{item.file_type}</span>
                          )}
                        </div>
                        <div className="text-[11px] font-mono text-on-surface/50 truncate" title={item.path}>
                          {parentPath(item.path)}
                        </div>
                        <p className={`text-xs text-on-surface/60 leading-relaxed ${open ? '' : 'line-clamp-1'}`}>
                          {item.description}
                        </p>
                        {item.blocked_by && (
                          <p className="flex items-center gap-1.5 text-xs font-medium text-error">
                            <Lock className="w-3 h-3" /> Close {item.blocked_by} to remove this item.
                          </p>
                        )}
                      </div>

                      <div className="flex flex-col items-end gap-1.5 min-w-[150px]">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-mono font-medium tabular-nums whitespace-nowrap">{formatBytes(item.size_bytes)}</span>
                          <span className={`text-[10px] font-semibold px-1.5 py-0.5 rounded border uppercase tracking-wide ${style.badge}`}>
                            {style.label}
                          </span>
                        </div>
                        <div className="w-full h-1 rounded-full bg-surface-bright overflow-hidden" aria-hidden>
                          <div className={`h-full ${style.dot} opacity-70`} style={{ width: `${Math.max(2, (item.size_bytes / maxSize) * 100)}%` }} />
                        </div>
                        <div className="flex items-center gap-0.5 text-on-surface/50" onClick={e => e.stopPropagation()}>
                          <button onClick={() => reveal(item)} title="Show in Finder" className="p-1.5 rounded-md hover:bg-surface-bright hover:text-on-surface">
                            <FolderSearch className="w-3.5 h-3.5" />
                          </button>
                          <button onClick={() => handleIgnore(item)} title="Never show this path again" className="p-1.5 rounded-md hover:bg-surface-bright hover:text-error">
                            <EyeOff className="w-3.5 h-3.5" />
                          </button>
                          <button
                            onClick={() => setExpanded(prev => { const n = new Set(prev); if (n.has(item.id)) n.delete(item.id); else n.add(item.id); return n; })}
                            title={open ? 'Less detail' : 'More detail'}
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
  );
}
