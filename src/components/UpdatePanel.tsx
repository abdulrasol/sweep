import { RefreshCw, Download } from 'lucide-react';
import { openExternal } from '../lib/tauri';
import type { UpdateState } from '../lib/updates';

interface UpdatePanelProps {
  update: UpdateState;
  onCheck: () => void;
}

/** Update status with the matching action. Shared by Settings and About. */
export default function UpdatePanel({ update, onCheck }: UpdatePanelProps) {
  const status =
    update.kind === 'checking' ? 'Checking for updates…'
    : update.kind === 'current' ? 'You have the latest version.'
    : update.kind === 'available' ? `Sweep ${update.version} is available.`
    : update.kind === 'error' ? 'Could not reach the update server. Check your connection and try again.'
    : 'Sweep does not update itself. It tells you when a new version is out.';

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
              What's new
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
          Download {update.version}
        </button>
      ) : (
        <button
          onClick={onCheck}
          disabled={update.kind === 'checking'}
          className="flex items-center gap-2 px-4 py-2 rounded-lg border border-outline text-sm font-medium hover:bg-surface-bright transition-colors disabled:opacity-50"
        >
          <RefreshCw className={`w-4 h-4 ${update.kind === 'checking' ? 'animate-spin' : ''}`} />
          Check for updates
        </button>
      )}
    </div>
  );
}
