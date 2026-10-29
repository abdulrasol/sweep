import { useEffect, useMemo, useState } from 'react';
import { invoke, formatBytes } from '../lib/tauri';
import { moduleTitle } from '../lib/modules';
import { useI18n } from '../i18n';

const DAY_MS = 86_400_000;
const RECENT_DAYS = 30;

/** The last 30 UTC days, oldest first, with zeros for days without removals. */
function lastDays(byDay: HistoryDay[], nowMs: number): HistoryDay[] {
  const known = new Map(byDay.map(d => [d.day, d]));
  const days: HistoryDay[] = [];
  for (let i = RECENT_DAYS - 1; i >= 0; i--) {
    const day = new Date(nowMs - i * DAY_MS).toISOString().slice(0, 10);
    days.push(known.get(day) ?? { day, freed: 0, trashed: 0, count: 0 });
  }
  return days;
}

export default function HistoryView() {
  const { t, td, tn, lang } = useI18n();
  const [summary, setSummary] = useState<HistorySummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    let alive = true;
    invoke<HistorySummary>('get_history')
      .then(data => { if (alive) setSummary(data); })
      .catch(err => { if (alive) setError(String(err)); });
    return () => { alive = false; };
  }, []);

  const days = useMemo(() => lastDays(summary?.by_day ?? [], Date.now()), [summary]);
  const maxDay = Math.max(1, ...days.map(d => d.freed + d.trashed));
  const maxModule = Math.max(1, ...(summary?.by_module ?? []).map(m => m.freed + m.trashed));

  const clear = async () => {
    if (!confirming) {
      setConfirming(true);
      return;
    }
    setConfirming(false);
    try {
      await invoke<void>('clear_history');
      setSummary(await invoke<HistorySummary>('get_history'));
      setError(null);
    } catch (err) {
      setError(typeof err === 'string' ? err : String(err));
    }
  };

  if (error && !summary) {
    return (
      <div className="max-w-3xl mx-auto px-6 py-8">
        <p className="text-sm text-error">{t('history.readError', { error })}</p>
      </div>
    );
  }
  if (!summary) {
    return (
      <div className="max-w-3xl mx-auto px-6 py-8">
        <p className="text-sm text-on-surface/60">{t('history.loading')}</p>
      </div>
    );
  }

  const [freedValue, freedUnit] = formatBytes(summary.total_freed).split(' ');
  const since = summary.first_ts !== null
    ? new Date(summary.first_ts * 1000).toLocaleDateString(lang === 'ar' ? 'ar-u-nu-latn' : undefined, { year: 'numeric', month: 'short', day: 'numeric' })
    : null;

  return (
    <div className="max-w-3xl mx-auto px-6 py-8 space-y-6">
      <header className="space-y-1">
        <h2 className="text-2xl font-semibold tracking-tight">{t('history.title')}</h2>
        <p className="text-sm text-on-surface/60">{t('history.intro')}</p>
      </header>

      {summary.items_removed === 0 ? (
        <section className="bg-surface-container border border-outline rounded-2xl p-6">
          <p className="text-sm text-on-surface/60">{t('history.empty')}</p>
        </section>
      ) : (
        <>
          <section className="bg-surface-container border border-outline rounded-2xl p-6 space-y-1">
            <p className="text-xs uppercase tracking-wide text-on-surface/50">{t('history.freedSoFar')}</p>
            <div className="flex items-baseline gap-1">
              <span className="text-5xl font-light tracking-tight tabular-nums text-primary">{freedValue}</span>
              <span className="text-base text-on-surface/60">{freedUnit}</span>
            </div>
            <p className="text-sm text-on-surface/60 tabular-nums">
              {tn('history.removed', summary.items_removed)}{since ? t('history.since', { date: since }) : ''}
            </p>
            <p className="text-sm text-on-surface/60 tabular-nums">{t('history.last30', { size: formatBytes(summary.last_30_days_freed) })}</p>
            {summary.total_trashed > 0 && (
              <p className="text-xs text-on-surface/50 tabular-nums">
                {t('history.inTrash', { size: formatBytes(summary.total_trashed) })}
              </p>
            )}
          </section>

          <section className="bg-surface-container border border-outline rounded-2xl p-6 space-y-3">
            <h3 className="text-sm font-semibold">{t('history.last30Title')}</h3>
            <div className="flex items-end gap-px h-24" dir="ltr" role="img" aria-label={t('history.chartLabel')}>
              {days.map(d => {
                const total = d.freed + d.trashed;
                const label = `${d.day}: ${total > 0 ? formatBytes(total) : t('history.nothingRemoved')}`;
                return (
                  <div key={d.day} title={label} className="flex-1 h-full flex flex-col justify-end">
                    {d.trashed > 0 && (
                      <div className="bg-primary/40 rounded-t-sm" style={{ height: `${(d.trashed / maxDay) * 100}%`, minHeight: 2 }} />
                    )}
                    {d.freed > 0 && (
                      <div className={`bg-primary ${d.trashed > 0 ? '' : 'rounded-t-sm'}`} style={{ height: `${(d.freed / maxDay) * 100}%`, minHeight: 2 }} />
                    )}
                  </div>
                );
              })}
            </div>
            <div className="flex justify-between text-[11px] text-on-surface/45 tabular-nums">
              <span>{days[0].day}</span>
              <span>{days[days.length - 1].day}</span>
            </div>
          </section>

          <section className="bg-surface-container border border-outline rounded-2xl p-6 space-y-3">
            <h3 className="text-sm font-semibold">{t('history.byTool')}</h3>
            <ul className="space-y-2.5">
              {summary.by_module.map(m => {
                const total = m.freed + m.trashed;
                return (
                  <li key={m.module} className="space-y-1">
                    <div className="flex items-baseline justify-between gap-4 text-sm">
                      <span className="font-medium truncate">{td(`module.${m.module}.title`, moduleTitle(m.module))}</span>
                      <span className="text-on-surface/60 tabular-nums shrink-0">
                        {formatBytes(total)} · {tn('review.items', m.count)}
                      </span>
                    </div>
                    <div className="h-1.5 rounded-full bg-surface-bright overflow-hidden">
                      <div className="h-full rounded-full bg-primary" style={{ width: `${(total / maxModule) * 100}%` }} />
                    </div>
                  </li>
                );
              })}
            </ul>
          </section>

          <div className="flex items-center gap-3">
            <button
              onClick={clear}
              className={`px-3 py-1.5 rounded-lg border text-sm transition-colors ${confirming ? 'border-error text-error' : 'border-outline text-on-surface/70 hover:text-on-surface'}`}
            >
              {confirming ? t('history.clearConfirm') : t('history.clear')}
            </button>
            {confirming && (
              <button onClick={() => setConfirming(false)} className="text-sm text-on-surface/60 hover:text-on-surface">
                {t('history.cancel')}
              </button>
            )}
            {error && <span className="text-sm text-error">{error}</span>}
          </div>
        </>
      )}
    </div>
  );
}
