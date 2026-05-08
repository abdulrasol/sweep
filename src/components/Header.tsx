import { Terminal, Settings, Share2, Monitor, Cpu } from 'lucide-react';

interface HeaderProps {
  title: string;
}

export default function Header({ title }: HeaderProps) {
  return (
    <header className="h-16 border-b border-outline bg-surface-container flex items-center justify-between px-6 shrink-0 z-10 w-full">
      <div className="flex items-center gap-4">
        <div className="flex items-center gap-2 text-on-surface/40">
          <span className="text-[10px] font-bold uppercase tracking-widest hover:text-on-surface cursor-pointer transition-colors">Core</span>
          <span className="text-[10px] opacity-30">/</span>
          <span className="text-xs font-semibold text-on-surface">{title}</span>
        </div>
      </div>

      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2 px-3 py-1.5 bg-primary/5 border border-primary/20 rounded-full">
          <div className="w-2 h-2 bg-primary rounded-full animate-pulse shadow-[0_0_8px_rgba(16,185,129,0.5)]"></div>
          <span className="text-[10px] text-primary font-bold uppercase tracking-tighter">System Engine Active</span>
        </div>
        <div className="w-px h-6 bg-outline mx-2"></div>
        <div className="flex gap-1">
          {[Terminal, Cpu, Settings].map((Icon, idx) => (
            <button key={idx} className="p-2 text-on-surface/40 hover:text-on-surface hover:bg-surface-bright rounded-md transition-all">
              <Icon className="w-4 h-4" />
            </button>
          ))}
        </div>
      </div>
    </header>
  );
}
