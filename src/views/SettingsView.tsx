import type { ReactNode } from 'react';
import { Moon, Sun, Eye } from 'lucide-react';
import { ACCENTS } from '../lib/accents';
import UpdatePanel from '../components/UpdatePanel';
import type { UpdateState } from '../lib/updates';
import { SEARCH_DEPTHS } from '../lib/scanDepth';
import { useI18n, LANGUAGES } from '../i18n';

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
  const { t, td, lang, setLang } = useI18n();
  return (
    <div className="max-w-3xl mx-auto px-6 py-6 space-y-6">
      <header className="space-y-1">
        <h2 className="text-xl font-semibold tracking-tight">{t('settings.title')}</h2>
        <p className="text-sm text-on-surface/60">{t('settings.saved')}</p>
      </header>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">{t('settings.appearance')}</h3>
        <div className="bg-surface-container border border-outline rounded-xl divide-y divide-outline">
          <Row title={t('settings.language')} hint={t('settings.languageHint')}>
            <div className="flex rounded-lg border border-outline p-0.5 bg-surface-dim" role="radiogroup" aria-label={t('settings.language')}>
              {LANGUAGES.map(l => (
                <button
                  key={l.id}
                  role="radio"
                  aria-checked={lang === l.id}
                  lang={l.id}
                  onClick={() => setLang(l.id)}
                  className={`px-3 py-1.5 rounded-md text-sm transition-colors
                    ${lang === l.id ? 'bg-surface-container shadow-sm font-medium' : 'text-on-surface/60 hover:text-on-surface'}`}
                >
                  {l.label}
                </button>
              ))}
            </div>
          </Row>
          <Row title={t('settings.theme')} hint={t('settings.themeHint')}>
            <div className="flex rounded-lg border border-outline p-0.5 bg-surface-dim" role="radiogroup" aria-label={t('settings.theme')}>
              {(['light', 'dark'] as const).map(th => (
                <button
                  key={th}
                  role="radio"
                  aria-checked={theme === th}
                  onClick={() => onThemeChange(th)}
                  className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm transition-colors
                    ${theme === th ? 'bg-surface-container shadow-sm font-medium' : 'text-on-surface/60 hover:text-on-surface'}`}
                >
                  {th === 'light' ? <Sun className="w-3.5 h-3.5" /> : <Moon className="w-3.5 h-3.5" />}
                  {th === 'light' ? t('settings.light') : t('settings.dark')}
                </button>
              ))}
            </div>
          </Row>
          <Row title={t('settings.accent')} hint={t('settings.accentHint')}>
            <div className="flex gap-1.5" role="radiogroup" aria-label={t('settings.accent')}>
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
                  {td(`accent.${c.id}`, c.label)}
                </button>
              ))}
            </div>
          </Row>
        </div>
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">{t('settings.scanning')}</h3>
        <div className="bg-surface-container border border-outline rounded-xl divide-y divide-outline">
          <Row
            title={t('settings.depth')}
            hint={t('settings.depthHint')}
          >
            <select
              aria-label={t('settings.depth')}
              value={maxDepth}
              onChange={(e) => onMaxDepthChange(Number(e.target.value))}
              className="bg-surface-dim border border-outline rounded-lg px-3 py-1.5 text-sm outline-none focus:border-primary/60"
            >
              {SEARCH_DEPTHS.map(depth => (
                <option key={depth} value={depth}>{t('settings.levels', { n: depth })}</option>
              ))}
            </select>
          </Row>
        </div>
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">{t('settings.removal')}</h3>
        <div className="bg-surface-container border border-outline rounded-xl divide-y divide-outline">
          <Row
            title={t('settings.whenRemoving')}
            hint={t('settings.whenRemovingHint')}
          >
            <div className="flex rounded-lg border border-outline p-0.5 bg-surface-dim" role="radiogroup" aria-label={t('settings.whenRemoving')}>
              {(['trash', 'permanent'] as const).map(mode => (
                <button
                  key={mode}
                  role="radio"
                  aria-checked={deleteMode === mode}
                  onClick={() => onDeleteModeChange(mode)}
                  className={`px-3 py-1.5 rounded-md text-sm transition-colors
                    ${deleteMode === mode ? 'bg-surface-container shadow-sm font-medium' : 'text-on-surface/60 hover:text-on-surface'}`}
                >
                  {mode === 'trash' ? t('settings.toTrash') : t('settings.permanent')}
                </button>
              ))}
            </div>
          </Row>
        </div>
      </section>

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">{t('settings.updates')}</h3>
        <div className="bg-surface-container border border-outline rounded-xl divide-y divide-outline">
          <Row title={t('settings.checkAtLaunch')} hint={t('settings.checkAtLaunchHint')}>
            <div className="flex rounded-lg border border-outline p-0.5 bg-surface-dim" role="radiogroup" aria-label={t('settings.checkAtLaunchLabel')}>
              {([true, false] as const).map(on => (
                <button
                  key={String(on)}
                  role="radio"
                  aria-checked={autoUpdateCheck === on}
                  onClick={() => onAutoUpdateCheckChange(on)}
                  className={`px-3 py-1.5 rounded-md text-sm transition-colors
                    ${autoUpdateCheck === on ? 'bg-surface-container shadow-sm font-medium' : 'text-on-surface/60 hover:text-on-surface'}`}
                >
                  {on ? t('settings.on') : t('settings.off')}
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
            <h3 className="text-sm font-semibold">{t('settings.hidden')}</h3>
            <p className="text-xs text-on-surface/55">{t('settings.hiddenHint')}</p>
          </div>
          {ignoredPaths.length > 1 && (
            <button onClick={onUnhideAll} className="text-sm text-primary hover:underline shrink-0">{t('settings.showAll')}</button>
          )}
        </div>
        <div className="bg-surface-container border border-outline rounded-xl">
          {ignoredPaths.length === 0 ? (
            <p className="px-5 py-4 text-sm text-on-surface/55">
              {t('settings.nothingHidden')}
            </p>
          ) : (
            <ul className="divide-y divide-outline">
              {ignoredPaths.map(p => (
                <li key={p} className="flex items-center gap-3 px-5 py-2.5">
                  <span className="flex-1 min-w-0 font-mono text-xs truncate text-start" dir="ltr" title={p}>{tildePath(p)}</span>
                  <button
                    onClick={() => onUnhide(p)}
                    className="flex items-center gap-1.5 text-xs text-on-surface/60 hover:text-on-surface shrink-0"
                  >
                    <Eye className="w-3.5 h-3.5" />
                    {t('settings.showAgain')}
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
