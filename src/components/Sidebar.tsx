import { Home, Scan, FileSearch, Settings, Database, Activity, LayoutGrid, Terminal, Info } from 'lucide-react';
import { motion } from 'motion/react';

interface SidebarProps {
  activeView: string;
  onViewChange: (view: string) => void;
}

export default function Sidebar({ activeView, onViewChange }: SidebarProps) {
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
        <div className="w-8 h-8 bg-outline-variant rounded-md flex items-center justify-center text-xs font-bold border border-outline shadow-sm">
          <Database className="w-4 h-4 text-primary" />
        </div>
        <div className="flex flex-col">
          <h1 className="text-sm font-semibold tracking-tight text-on-surface">DevClean</h1>
          <p className="text-[10px] text-on-surface/40 uppercase tracking-widest font-mono">v1.0.4-stable</p>
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
                  ? 'bg-outline-variant/50 border-outline-variant text-on-surface' 
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
        <div className="p-4 bg-surface-bright/20 rounded-xl border border-outline">
          <div className="text-[10px] text-on-surface/40 uppercase font-bold tracking-widest mb-3">Runtime Status</div>
          <div className="space-y-2">
            <div className="flex justify-between text-[11px] font-mono">
              <span className="text-on-surface/40">Engine:</span>
              <span className="text-on-surface/80">Tauri v1.5</span>
            </div>
            <div className="flex justify-between text-[11px] font-mono">
              <span className="text-on-surface/40">Memory:</span>
              <span className="text-primary font-bold">42.4 MB</span>
            </div>
          </div>
        </div>
      </div>
    </aside>
  );
}
