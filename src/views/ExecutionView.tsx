import { useEffect, useRef, useState } from 'react';
import { Check, X, Loader2, Square, RefreshCw, FolderCog } from 'lucide-react';
import { invoke, formatBytes } from '../lib/tauri';
import { useI18n } from '../i18n';
import { clearDiskMapCache } from '../lib/diskMapCache';

interface ExecutionViewProps {
  items: CleanupItem[];
  deleteMode: 'trash' | 'permanent';
  onRunningChange: (running: boolean) => void;
  onScanAgain: () => void;
  onChangeScope: () => void;
}

type ItemState =
  | { kind: 'pending' }
  | { kind: 'working' }
  | { kind: 'done'; freed: number; trashed: boolean }
  | { kind: 'failed'; error: string; freed: number }
  | { kind: 'skipped' };

type Phase = 'running' | 'stopping' | 'stopped' | 'done';

const tildePath = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');
const name = (path: string) => path.split('/').filter(Boolean).pop() ?? path;
const parent = (path: string) => tildePath(path.split('/').slice(0, -1).join('/') || '/');

export default function ExecutionView({ items, deleteMode, onRunningChange, onScanAgain, onChangeScope }: ExecutionViewProps) {
  const { t, tn } = useI18n();
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
          const result = await invoke<{ bytes: number; trashed: boolean }>('cleanup_item', {
            id: items[i].id,
            mode: deleteMode,
          });
          set(i, { kind: 'done', freed: result.bytes, trashed: result.trashed });
        } catch (err) {
          const failure = err as { message?: unknown; freed?: unknown } | null;
          if (
            failure &&
            typeof failure.message === 'string' &&
            typeof failure.freed === 'number'
          ) {
            set(i, { kind: 'failed', error: failure.message, freed: failure.freed });
          } else {
            set(i, { kind: 'failed', error: String(err), freed: 0 });
          }
        }
      }
      // Sizes saved for the disk map are out of date after a removal.
      clearDiskMapCache();
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

  const freed = states.reduce(
    (sum, s) =>
      sum +
      (s.kind === 'done' && !s.trashed ? s.freed : 0) +
      (s.kind === 'failed' ? s.freed : 0),
    0,
  );
  const trashedBytes = states.reduce(
    (sum, s) => sum + (s.kind === 'done' && s.trashed ? s.freed : 0),
    0,
  );
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
    phase === 'done' ? (failed ? tn('exec.problems', failed) : t('exec.finished'))
    : phase === 'stopped' ? t('exec.stopped')
    : phase === 'stopping' ? t('exec.stoppingTitle')
    : tn('exec.cleaning', total);

  const subtitle = running
    ? t('exec.removing', { path: tildePath(items[current]?.path ?? '') })
    : tn('exec.removed', total, { done: doneCount }) + (failed ? t('exec.failedNote') : '');

  const [freedValue, freedUnit] = formatBytes(freed).split(' ');

  return (
    <div className="max-w-4xl mx-auto px-6 py-6 flex flex-col gap-4 h-full">
      <section className="bg-surface-container border border-outline rounded-2xl p-6 space-y-5 shrink-0">
        <div className="flex items-start justify-between gap-6">
          <div className="min-w-0 space-y-1">
            <h2 className="text-xl font-semibold tracking-tight">{title}</h2>
            <p className="text-sm text-on-surface/60 truncate" title={subtitle}>{subtitle}</p>
          </div>
          <div className="text-end shrink-0" aria-live="polite">
            <div className="flex items-baseline justify-end gap-1">
              <span className="text-5xl font-light tracking-tight tabular-nums text-primary">{freedValue}</span>
              <span className="text-base text-on-surface/60">{freedUnit}</span>
            </div>
            <div className="text-xs text-on-surface/50 tabular-nums">
              {t('exec.freedOf', { size: formatBytes(selectedBytes) })}
            </div>
            {trashedBytes > 0 && (
              <div className="text-xs text-on-surface/50">
                {t('exec.trashed', { size: formatBytes(trashedBytes) })}
              </div>
            )}
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
            <span>{failed ? t('exec.progressFailed', { done: finished, total, failed }) : t('exec.progress', { done: finished, total })}</span>
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
              {t('exec.stopAfter')}
            </button>
          ) : (
            <>
              <button
                onClick={onChangeScope}
                className="flex items-center gap-2 px-4 py-2 rounded-lg border border-outline text-sm font-medium hover:bg-surface-bright transition-colors"
              >
                <FolderCog className="w-4 h-4" />
                {t('exec.changeScope')}
              </button>
              <button
                onClick={onScanAgain}
                className="flex items-center gap-2 px-4 py-2 rounded-lg bg-on-surface text-surface text-sm font-medium hover:opacity-90 transition-opacity"
              >
                <RefreshCw className="w-4 h-4" />
                {t('exec.scanAgain')}
              </button>
            </>
          )}
        </div>
      </section>

      <section className="flex-1 min-h-[240px] bg-surface-container border border-outline rounded-2xl overflow-hidden flex flex-col">
        <div className="px-4 py-2.5 border-b border-outline text-xs text-on-surface/60 flex justify-between">
          <span>{t('exec.listTitle')}</span>
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
                    <span className="ms-2 text-xs font-normal text-on-surface/45">{item.file_type || item.type}</span>
                  </div>
                  <div className="text-[11px] font-mono text-on-surface/45 truncate" title={item.path}>{parent(item.path)}</div>
                  {s.kind === 'failed' && (
                    <p className="mt-1 text-xs text-error">
                      {s.freed > 0
                        ? t('exec.partly', { size: formatBytes(s.freed), error: s.error })
                        : s.error}
                    </p>
                  )}
                </div>
                <span className={`text-sm font-mono tabular-nums whitespace-nowrap ${s.kind === 'done' ? '' : 'text-on-surface/50'}`}>
                  {s.kind === 'skipped' ? t('exec.skipped')
                    : s.kind === 'failed' && s.freed > 0 ? formatBytes(s.freed)
                    : formatBytes(item.size_bytes)}
                </span>
                <span className="sr-only">
                  {s.kind === 'done' ? (s.trashed ? t('exec.inTrash') : t('exec.removedState')) : s.kind === 'failed' ? (s.freed > 0 ? t('exec.partlyState') : t('exec.failedState')) : s.kind === 'working' ? t('exec.removingState') : s.kind === 'skipped' ? t('exec.skipped') : t('exec.waiting')}
                </span>
              </li>
            );
          })}
        </ol>
      </section>
    </div>
  );
}
