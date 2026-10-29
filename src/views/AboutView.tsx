import { Globe, Mail, Github } from 'lucide-react';
import UpdatePanel from '../components/UpdatePanel';
import type { UpdateState } from '../lib/updates';
import { useI18n } from '../i18n';

const LINKS = [
  { icon: Globe, label: 'about.website', value: 'sweep.subultech.top', href: 'https://sweep.subultech.top' },
  { icon: Github, label: 'GitHub', value: 'github.com/abdulrasol', href: 'https://github.com/abdulrasol' },
  { icon: Mail, label: 'about.email', value: 'abdulrsol97@gmail.com', href: 'mailto:abdulrsol97@gmail.com' },
];

interface AboutViewProps {
  version: string;
  update: UpdateState;
  onCheckForUpdates: () => void;
}

export default function AboutView({ version, update, onCheckForUpdates }: AboutViewProps) {
  const { t, td } = useI18n();
  return (
    <div className="max-w-3xl mx-auto px-6 py-8 space-y-6">
      <header className="flex items-center gap-4">
        <img src="/sweep.svg" alt="" className="w-16 h-16 rounded-2xl border border-outline object-cover" />
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">Sweep</h2>
          <p className="text-sm text-on-surface/60">
            {t('about.tagline')}
          </p>
          <p className="text-xs text-on-surface/45 tabular-nums mt-0.5">{t('about.version', { version: version || '–' })}</p>
        </div>
      </header>

      <section className="bg-surface-container border border-outline rounded-xl px-5 py-4">
        <UpdatePanel update={update} onCheck={onCheckForUpdates} />
      </section>

      <section className="grid md:grid-cols-[1fr_240px] gap-4">
        <div className="bg-surface-container border border-outline rounded-xl p-5 space-y-3 text-sm text-on-surface/70 leading-relaxed">
          <h3 className="text-sm font-semibold text-on-surface">{t('about.why')}</h3>
          <p>
            {t('about.story1a')}
            <code dir="ltr" className="font-mono text-xs px-1 py-0.5 rounded bg-surface-bright">flutter run ios</code>
            {t('about.story1b')}<strong className="text-on-surface">{t('about.story1c')}</strong>{t('about.story1d')}
          </p>
          <p>
            {t('about.story2')}
          </p>
          <p className="text-on-surface/55">
            {t('about.story3')}
          </p>
        </div>

        <div className="bg-surface-container border border-outline rounded-xl p-5 space-y-3">
          <div>
            <h3 className="text-sm font-semibold">Abdulrasol Al-Hilo</h3>
            <p className="text-xs text-on-surface/55">{t('about.role')}</p>
          </div>
          <ul className="space-y-1">
            {LINKS.map(l => (
              <li key={l.label}>
                <a
                  href={l.href}
                  className="flex items-center gap-2.5 -mx-2 px-2 py-1.5 rounded-md text-sm text-on-surface/70 hover:text-on-surface hover:bg-surface-bright transition-colors"
                >
                  <l.icon className="w-4 h-4 shrink-0" />
                  <span className="truncate" dir="ltr">{l.value}</span>
                  <span className="sr-only">({td(l.label, l.label)})</span>
                </a>
              </li>
            ))}
          </ul>
        </div>
      </section>

      <p className="text-xs text-on-surface/40 text-center">{t('about.madeIn')}</p>
    </div>
  );
}
