import { Settings, Info, HardDrive } from 'lucide-react';
import { formatBytes } from '../lib/tauri';

interface SidebarProps {
  activeView: View;
  onNavigate: (view: View) => void;
  /** True while a cleanup runs: navigation waits until it stops. */
  locked: boolean;
  sysInfo: SystemInfo | null;
}

// Scan, Review and Clean are the three steps of one run, so they are numbered.
const STEPS: { id: View; label: string; hint: string }[] = [
  { id: 'scan', label: 'Scan', hint: 'Pick a folder and tools' },
  { id: 'review', label: 'Review', hint: 'Choose what to remove' },
  { id: 'cleanup', label: 'Clean', hint: 'Remove and free space' },
];

const EXTRA: { id: View; label: string; icon: typeof Settings }[] = [
  { id: 'settings', label: 'Settings', icon: Settings },
  { id: 'about', label: 'About', icon: Info },
];

export default function Sidebar({ activeView, onNavigate, locked, sysInfo }: SidebarProps) {
  const usedRatio = sysInfo && sysInfo.disk_total > 0 ? 1 - sysInfo.disk_free / sysInfo.disk_total : 0;
  const lowSpace = sysInfo ? sysInfo.disk_free < 20e9 : false;

  return (
    <aside className="w-56 bg-surface-container border-r border-outline flex flex-col h-full shrink-0">
      <div className="px-5 pt-5 pb-6 flex items-center gap-3">
        <img src="/sweep.png" alt="" className="w-9 h-9 rounded-[9px] object-cover border border-outline" />
        <div className="leading-tight">
          <h1 className="text-[15px] font-semibold">Sweep</h1>
          <p className="text-xs text-on-surface/50">Reclaim your storage</p>
        </div>
      </div>

      <nav className="flex-1 px-3 overflow-y-auto" aria-label="Main">
        <ol className="space-y-0.5">
          {STEPS.map((step, i) => {
            const active = activeView === step.id;
            const disabled = locked && !active;
            return (
              <li key={step.id}>
                <button
                  onClick={() => onNavigate(step.id)}
                  disabled={disabled}
                  aria-current={active ? 'step' : undefined}
                  className={`w-full flex items-center gap-3 px-2.5 py-2 rounded-lg text-left transition-colors
                    ${active ? 'bg-surface-bright' : 'hover:bg-surface-bright/60'}
                    ${disabled ? 'opacity-40 cursor-not-allowed' : ''}`}
                >
                  <span
                    className={`w-6 h-6 shrink-0 rounded-full grid place-items-center text-xs font-semibold tabular-nums border transition-colors
                      ${active ? 'bg-primary border-primary text-on-primary' : 'border-outline-variant text-on-surface/60'}`}
                  >
                    {i + 1}
                  </span>
                  <span className="min-w-0">
                    <span className={`block text-sm ${active ? 'font-semibold' : 'font-medium text-on-surface/80'}`}>{step.label}</span>
                    <span className="block text-[11px] text-on-surface/45 truncate">{step.hint}</span>
                  </span>
                </button>
              </li>
            );
          })}
        </ol>

        <div className="my-3 mx-2.5 h-px bg-outline" />

        <ul className="space-y-0.5">
          {EXTRA.map(item => {
            const active = activeView === item.id;
            return (
              <li key={item.id}>
                <button
                  onClick={() => onNavigate(item.id)}
                  disabled={locked}
                  aria-current={active ? 'page' : undefined}
                  className={`w-full flex items-center gap-3 px-2.5 py-1.5 rounded-lg text-sm transition-colors
                    ${active ? 'bg-surface-bright font-semibold' : 'text-on-surface/70 hover:bg-surface-bright/60 hover:text-on-surface'}
                    ${locked ? 'opacity-40 cursor-not-allowed' : ''}`}
                >
                  <item.icon className={`w-4 h-4 mx-1 ${active ? 'text-primary' : ''}`} />
                  {item.label}
                </button>
              </li>
            );
          })}
        </ul>
      </nav>

      <div className="p-3">
        <div className="p-3.5 rounded-xl border border-outline bg-surface-dim space-y-2.5">
          <div className="flex items-center gap-2 text-xs text-on-surface/60">
            <HardDrive className="w-3.5 h-3.5" />
            Startup disk
          </div>
          <div>
            <div className={`text-lg font-semibold tabular-nums leading-tight ${lowSpace ? 'text-error' : ''}`}>
              {sysInfo ? formatBytes(sysInfo.disk_free) : '–'}
              <span className="text-xs font-normal text-on-surface/50"> free</span>
            </div>
            <div className="text-[11px] text-on-surface/45 tabular-nums">
              {sysInfo ? `of ${formatBytes(sysInfo.disk_total)}` : 'Reading disk…'}
            </div>
          </div>
          <div
            className="h-1.5 rounded-full bg-surface-bright overflow-hidden"
            role="meter"
            aria-label="Disk used"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(usedRatio * 100)}
          >
            <div
              className={`h-full rounded-full transition-[width] duration-700 ${lowSpace ? 'bg-error' : 'bg-primary'}`}
              style={{ width: `${usedRatio * 100}%` }}
            />
          </div>
          <div className="pt-2 border-t border-outline flex justify-between text-[11px] text-on-surface/50 tabular-nums">
            <span>CPU {sysInfo ? `${Math.round(sysInfo.cpu_usage)}%` : '–'}</span>
            <span>Memory {sysInfo ? formatBytes(sysInfo.ram_used) : '–'}</span>
          </div>
        </div>
      </div>
    </aside>
  );
}
