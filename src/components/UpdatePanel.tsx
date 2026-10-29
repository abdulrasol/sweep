import { RefreshCw, Download } from 'lucide-react';
import { openExternal } from '../lib/tauri';
import type { UpdateState } from '../lib/updates';
import { useI18n } from '../i18n';

interface UpdatePanelProps {
  update: UpdateState;
  onCheck: () => void;
}

/** Update status with the matching action. Shared by Settings and About. */
export default function UpdatePanel({ update, onCheck }: UpdatePanelProps) {
  const { t } = useI18n();
  const status =
    update.kind === 'checking' ? t('update.checking')
    : update.kind === 'current' ? t('update.current')
    : update.kind === 'available' ? t('update.available', { version: update.version })
    : update.kind === 'error' ? t('update.error')
    : t('update.idle');

  return (
    <div className="flex flex-wrap items-center gap-3">
      <p
        className={`flex-1 min-w-[220px] text-sm ${update.kind === 'error' ? 'text-error' : 'text-on-surface/70'}`}
        aria-live="polite"
      >
        {status}
        {update.kind === 'available' && (
          <>
            {' '}
            <button onClick={() => openExternal(update.releaseUrl)} className="text-primary hover:underline">
              {t('update.whatsNew')}
            </button>
          </>
        )}
      </p>
      {update.kind === 'available' ? (
        <button
          onClick={() => openExternal(update.downloadUrl)}
          className="flex items-center gap-2 px-4 py-2 rounded-lg bg-on-surface text-surface text-sm font-medium hover:opacity-90 transition-opacity"
        >
          <Download className="w-4 h-4" />
          {t('update.download', { version: update.version })}
        </button>
      ) : (
        <button
          onClick={onCheck}
          disabled={update.kind === 'checking'}
          className="flex items-center gap-2 px-4 py-2 rounded-lg border border-outline text-sm font-medium hover:bg-surface-bright transition-colors disabled:opacity-50"
        >
          <RefreshCw className={`w-4 h-4 ${update.kind === 'checking' ? 'animate-spin' : ''}`} />
          {t('update.check')}
        </button>
      )}
    </div>
  );
}
