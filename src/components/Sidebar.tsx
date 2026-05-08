import { Home, Scan, FileSearch, Settings, Database, Activity, LayoutGrid, Terminal, Info, Cpu, Monitor } from 'lucide-react';
import { motion } from 'motion/react';
interface SystemInfo {
  os_name: string;
  os_version: string;
  cpu_usage: number;
  ram_total: number;
  ram_used: number;
  disk_total: number;
  disk_free: number;
}

interface SidebarProps {
  activeView: string;
  onViewChange: (view: any) => void;
  sysInfo: SystemInfo | null;
}

export default function Sidebar({ activeView, onViewChange, sysInfo }: SidebarProps) {

  const navItems = [
    { id: 'scan', label: 'Scan Scope', icon: Scan },
    { id: 'review', label: 'Review Targets', icon: FileSearch },
    { id: 'cleanup', label: 'Purge Sequence', icon: Activity },
    { id: 'settings', label: 'Settings', icon: Settings },
    { id: 'about', label: 'About System', icon: Info },
  ];

  return (
    <aside className="w-60 bg-surface-container border-r border-outline flex flex-col h-full shrink-0">
      <div className="p-6 flex items-center gap-4 shrink-0">
        <div className="w-10 h-10 rounded-xl overflow-hidden border border-primary/40 shadow-2xl group transition-all duration-500 hover:border-primary/80 hover:shadow-primary/20">
          <img 
            src="/sweep.png" 
            alt="Sweep Logo" 
            className="w-full h-full object-cover"
            referrerPolicy="no-referrer"
          />
        </div>
        <div className="flex flex-col">
          <h1 className="text-base font-bold tracking-tight text-on-surface">Sweep</h1>
          <p className="text-[9px] text-primary/60 uppercase tracking-widest font-mono font-bold">RECLAIM YOUR STORAGE</p>
        </div>
      </div>

      <nav className="flex-1 px-4 space-y-1 overflow-y-auto">
        <div className="text-[10px] text-on-surface/40 uppercase font-bold tracking-widest mb-4 mt-2 px-2">Navigation</div>
        {navItems.map((item) => {
          const isActive = activeView === item.id;
          
          return (
            <button
              key={item.id}
              onClick={() => onViewChange(item.id)}
              className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg transition-all duration-200 group relative border
                ${isActive 
                  ? 'bg-outline-variant/50 border-outline-variant text-on-surface shadow-inner' 
                  : 'text-on-surface/50 border-transparent hover:text-on-surface hover:bg-surface-bright/50'
                }`}
            >
              <item.icon className={`w-4 h-4 transition-transform ${isActive ? 'text-primary scale-110' : 'group-hover:scale-105 opacity-70'}`} />
              <span className="text-sm font-medium">{item.label}</span>
            </button>
          );
        })}
      </nav>

      <div className="p-4 mt-auto">
        <div className="p-4 bg-surface-bright/20 rounded-xl border border-outline space-y-4">
          <div className="text-[10px] text-on-surface/40 uppercase font-bold tracking-widest text-center">Runtime Status</div>
          
          <div className="space-y-3">
            {/* CPU */}
            <div className="space-y-1.5">
              <div className="flex justify-between items-center text-[10px] font-mono">
                <div className="flex items-center gap-1.5 text-on-surface/40">
                  <Cpu className="w-3 h-3" />
                  <span>CPU</span>
                </div>
                <span className="text-on-surface/80 font-bold">{sysInfo ? `${sysInfo.cpu_usage.toFixed(1)}%` : '--'}</span>
              </div>
              <div className="w-full bg-surface-dim h-1 rounded-full overflow-hidden">
                <motion.div 
                  initial={{ width: 0 }}
                  animate={{ width: `${sysInfo ? Math.min(sysInfo.cpu_usage, 100) : 0}%` }}
                  className="bg-primary h-full" 
                />
              </div>
            </div>

            {/* RAM */}
            <div className="space-y-1.5">
              <div className="flex justify-between items-center text-[10px] font-mono">
                <div className="flex items-center gap-1.5 text-on-surface/40">
                  <Activity className="w-3 h-3" />
                  <span>RAM</span>
                </div>
                <span className="text-on-surface/80 font-bold">
                  {sysInfo ? `${(sysInfo.ram_used / 1024 / 1024 / 1024).toFixed(1)}GB` : '--'}
                </span>
              </div>
              <div className="w-full bg-surface-dim h-1 rounded-full overflow-hidden">
                <motion.div 
                  initial={{ width: 0 }}
                  animate={{ width: `${sysInfo ? (sysInfo.ram_used / sysInfo.ram_total) * 100 : 0}%` }}
                  className="bg-blue-500 h-full" 
                />
              </div>
            </div>

            {/* Disk */}
            <div className="space-y-1.5">
              <div className="flex justify-between items-center text-[10px] font-mono">
                <div className="flex items-center gap-1.5 text-on-surface/40">
                  <Database className="w-3 h-3" />
                  <span>Disk</span>
                </div>
                <span className="text-on-surface/80 font-bold">
                  {sysInfo ? `${(sysInfo.disk_free / 1024 / 1024 / 1024).toFixed(0)}GB` : '--'}
                </span>
              </div>
              <div className="w-full bg-surface-dim h-1 rounded-full overflow-hidden">
                <motion.div 
                  initial={{ width: 0 }}
                  animate={{ width: `${sysInfo ? (1 - sysInfo.disk_free / sysInfo.disk_total) * 100 : 0}%` }}
                  className="bg-orange-500 h-full" 
                />
              </div>
            </div>
          </div>

          <div className="pt-2 border-t border-outline/30 flex flex-col gap-1">
            <div className="flex items-center gap-1.5 text-[9px] text-on-surface/30 uppercase tracking-tighter">
              <Monitor className="w-2.5 h-2.5" />
              <span className="truncate">{sysInfo ? `${sysInfo.os_name} ${sysInfo.os_version}` : 'Detecting OS...'}</span>
            </div>
          </div>
        </div>
      </div>
    </aside>
  );
}
