import { useEffect, useRef, useState } from 'react';
import { Check, X, Loader2, Square, RefreshCw, FolderCog } from 'lucide-react';
import { invoke, formatBytes } from '../lib/tauri';

interface ExecutionViewProps {
  items: CleanupItem[];
  onRunningChange: (running: boolean) => void;
  onScanAgain: () => void;
  onChangeScope: () => void;
}

type ItemState =
  | { kind: 'pending' }
  | { kind: 'working' }
  | { kind: 'done'; freed: number }
  | { kind: 'failed'; error: string }
  | { kind: 'skipped' };

type Phase = 'running' | 'stopping' | 'stopped' | 'done';

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');
const name = (path: string) => path.split('/').filter(Boolean).pop() ?? path;
const parent = (path: string) => tildePath(path.split('/').slice(0, -1).join('/') || '/');

export default function ExecutionView({ items, onRunningChange, onScanAgain, onChangeScope }: ExecutionViewProps) {
  const [states, setStates] = useState<ItemState[]>(() => items.map(() => ({ kind: 'pending' })));
  const [phase, setPhase] = useState<Phase>('running');
  const [current, setCurrent] = useState(0);

  // React StrictMode runs effects twice in development. The ref keeps this run to one pass,
  // so no item is ever sent for deletion twice.
  const started = useRef(false);
  const stopRequested = useRef(false);
  const rowRefs = useRef<(HTMLLIElement | null)[]>([]);

  useEffect(() => {
    if (started.current) return;
    started.current = true;

    const set = (i: number, s: ItemState) =>
      setStates(prev => prev.map((old, j) => (j === i ? s : old)));

    const run = async () => {
      onRunningChange(true);
      for (let i = 0; i < items.length; i++) {
        if (stopRequested.current) {
          setStates(prev => prev.map((s, j) => (j >= i ? { kind: 'skipped' } : s)));
          break;
        }
        setCurrent(i);
        set(i, { kind: 'working' });
        try {
          const freed = await invoke<number>('cleanup_item', { id: items[i].id });
          set(i, { kind: 'done', freed });
        } catch (err) {
          set(i, { kind: 'failed', error: String(err) });
        }
      }
      setPhase(stopRequested.current ? 'stopped' : 'done');
      onRunningChange(false);
    };
    run();
    // The run starts once for the items it was given.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    rowRefs.current[current]?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }, [current]);

  const freed = states.reduce((sum, s) => sum + (s.kind === 'done' ? s.freed : 0), 0);
  const doneCount = states.filter(s => s.kind === 'done').length;
  const failed = states.filter(s => s.kind === 'failed').length;
  const finished = states.filter(s => s.kind === 'done' || s.kind === 'failed').length;
  const total = items.length;
  const selectedBytes = items.reduce((sum, i) => sum + i.size_bytes, 0);
  const running = phase === 'running' || phase === 'stopping';
  const progress = total ? finished / total : 0;

  const stop = () => {
    stopRequested.current = true;
    setPhase('stopping');
  };

  const title =
    phase === 'done' ? (failed ? `Finished with ${failed} ${failed === 1 ? 'problem' : 'problems'}` : 'Cleanup finished')
    : phase === 'stopped' ? 'Cleanup stopped'
    : phase === 'stopping' ? 'Stopping after this item…'
    : `Cleaning ${total} ${total === 1 ? 'item' : 'items'}`;

  const subtitle = running
    ? `Removing ${tildePath(items[current]?.path ?? '')}`
    : `Removed ${doneCount} of ${total} ${total === 1 ? 'item' : 'items'}.` +
      (failed ? ' The ones that failed are marked below with the reason.' : '');

  const [freedValue, freedUnit] = formatBytes(freed).split(' ');

  return (
    <div className="max-w-4xl mx-auto px-6 py-6 flex flex-col gap-4 h-full">
      <section className="bg-surface-container border border-outline rounded-2xl p-6 space-y-5 shrink-0">
        <div className="flex items-start justify-between gap-6">
          <div className="min-w-0 space-y-1">
            <h2 className="text-xl font-semibold tracking-tight">{title}</h2>
            <p className="text-sm text-on-surface/60 truncate" title={subtitle}>{subtitle}</p>
          </div>
          <div className="text-right shrink-0" aria-live="polite">
            <div className="flex items-baseline justify-end gap-1">
              <span className="text-5xl font-light tracking-tight tabular-nums text-primary">{freedValue}</span>
              <span className="text-base text-on-surface/60">{freedUnit}</span>
            </div>
            <div className="text-xs text-on-surface/50 tabular-nums">
              freed of {formatBytes(selectedBytes)} selected
            </div>
          </div>
        </div>

        <div className="space-y-2">
          <div
            className="h-2 rounded-full bg-surface-bright overflow-hidden"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={total}
            aria-valuenow={finished}
          >
            <div
              className={`h-full rounded-full transition-[width] duration-300 ${failed && !running ? 'bg-warning' : 'bg-primary'}`}
              style={{ width: `${Math.max(progress * 100, running ? 1.5 : 0)}%` }}
            />
          </div>
          <div className="flex justify-between text-xs text-on-surface/55 tabular-nums">
            <span>{finished} of {total} done{failed ? `, ${failed} failed` : ''}</span>
            <span>{Math.round(progress * 100)}%</span>
          </div>
        </div>

        <div className="flex flex-wrap justify-end gap-2">
          {running ? (
            <button
              onClick={stop}
              disabled={phase === 'stopping'}
              className="flex items-center gap-2 px-4 py-2 rounded-lg border border-outline text-sm font-medium hover:bg-surface-bright transition-colors disabled:opacity-50"
            >
              <Square className="w-3.5 h-3.5" />
              Stop after this item
            </button>
          ) : (
            <>
              <button
                onClick={onChangeScope}
                className="flex items-center gap-2 px-4 py-2 rounded-lg border border-outline text-sm font-medium hover:bg-surface-bright transition-colors"
              >
                <FolderCog className="w-4 h-4" />
                Change scope
              </button>
              <button
                onClick={onScanAgain}
                className="flex items-center gap-2 px-4 py-2 rounded-lg bg-on-surface text-surface text-sm font-medium hover:opacity-90 transition-opacity"
              >
                <RefreshCw className="w-4 h-4" />
                Scan again
              </button>
            </>
          )}
        </div>
      </section>

      <section className="flex-1 min-h-[240px] bg-surface-container border border-outline rounded-2xl overflow-hidden flex flex-col">
        <div className="px-4 py-2.5 border-b border-outline text-xs text-on-surface/60 flex justify-between">
          <span>Items, in the order they are removed</span>
          <span className="tabular-nums">{total}</span>
        </div>
        <ol className="flex-1 overflow-y-auto divide-y divide-outline">
          {items.map((item, i) => {
            const s = states[i];
            return (
              <li
                key={item.id}
                ref={el => { rowRefs.current[i] = el; }}
                className={`grid grid-cols-[20px_minmax(0,1fr)_auto] items-start gap-3 px-4 py-2.5
                  ${s.kind === 'working' ? 'bg-primary/[0.06]' : ''}`}
              >
                <span className="pt-0.5" aria-hidden>
                  {s.kind === 'done' && <Check className="w-4 h-4 text-primary" strokeWidth={2.5} />}
                  {s.kind === 'failed' && <X className="w-4 h-4 text-error" strokeWidth={2.5} />}
                  {s.kind === 'working' && <Loader2 className="w-4 h-4 text-primary animate-spin" />}
                  {(s.kind === 'pending' || s.kind === 'skipped') && (
                    <span className="block w-1.5 h-1.5 m-[5px] rounded-full bg-on-surface/25" />
                  )}
                </span>
                <div className="min-w-0">
                  <div className={`text-sm truncate ${s.kind === 'pending' || s.kind === 'skipped' ? 'text-on-surface/55' : 'font-medium'}`}>
                    {name(item.path)}
                    <span className="ml-2 text-xs font-normal text-on-surface/45">{item.file_type || item.type}</span>
                  </div>
                  <div className="text-[11px] font-mono text-on-surface/45 truncate" title={item.path}>{parent(item.path)}</div>
                  {s.kind === 'failed' && <p className="mt-1 text-xs text-error">{s.error}</p>}
                </div>
                <span className={`text-sm font-mono tabular-nums whitespace-nowrap ${s.kind === 'done' ? '' : 'text-on-surface/50'}`}>
                  {s.kind === 'skipped' ? 'Skipped' : formatBytes(item.size_bytes)}
                </span>
                <span className="sr-only">
                  {s.kind === 'done' ? 'Removed' : s.kind === 'failed' ? 'Failed' : s.kind === 'working' ? 'Removing' : s.kind === 'skipped' ? 'Skipped' : 'Waiting'}
                </span>
              </li>
            );
          })}
        </ol>
      </section>
    </div>
  );
}
