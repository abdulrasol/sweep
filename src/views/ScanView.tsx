import { useState } from 'react';
import { Folder, Edit, ChevronRight, Apple, Codepen, Container, HardDrive, Zap } from 'lucide-react';
import { motion } from 'motion/react';

interface ScanViewProps {
  onInitiate: (path: string) => void;
}

export default function ScanView({ onInitiate }: ScanViewProps) {
  const [path, setPath] = useState('/Users/developer/workspace/projects');

  const modules = [
    { id: 'xcode', title: 'Xcode Artifacts', icon: Apple, description: 'Targets DerivedData, Archives, and iOS Device Support files. High probability of reclaiming >10GB.', path: '~/Library/Developer/Xcode', enabled: true, color: 'text-blue-400' },
    { id: 'node', title: 'Node Ecosystem', icon: Codepen, description: 'Scans for orphaned node_modules directories and clears global npm/yarn cache binaries.', path: '~/npm, node_modules', enabled: true, color: 'text-yellow-400' },
    { id: 'docker', title: 'Docker Engine', icon: Container, description: 'Identifies dangling images, unused volumes, and stopped containers. Requires daemon running.', path: '/var/lib/docker', enabled: false, color: 'text-cyan-400' },
    { id: 'os', title: 'System OS Caches', icon: HardDrive, description: 'Aggressive cleaning of OS-level application caches, logs, and temporary user directories.', path: '~/Library/Caches', enabled: false, color: 'text-red-400', badge: 'Sudo Required' },
  ];

  return (
    <div className="max-w-5xl mx-auto py-8 px-6 space-y-8">
      <section className="space-y-2">
        <h2 className="text-2xl font-bold tracking-tight text-on-surface">Target Scope Configuration</h2>
        <p className="text-sm text-on-surface/40 leading-relaxed max-w-2xl">
          Define the root analysis scope and active modules. The orchestrator will verify dependencies and stale artifacts against the selected development frameworks.
        </p>
      </section>

      {/* Root Directory Scope */}
      <div className="bg-surface-container border border-outline rounded-2xl overflow-hidden shadow-sm">
        <div className="p-4 bg-surface-bright/30 border-b border-outline flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Folder className="w-4 h-4 text-primary opacity-80" />
            <span className="text-[10px] font-bold uppercase tracking-widest text-on-surface/60">Root Directory Scope</span>
          </div>
        </div>
        <div className="p-6 flex items-center gap-4">
          <div className="flex-1 flex items-center gap-3 bg-surface-dim border border-outline px-4 py-3 rounded-xl font-mono text-xs group focus-within:border-primary/50 transition-all text-on-surface/70">
            <Monitor className="w-4 h-4 opacity-30" />
            <input 
              value={path}
              onChange={(e) => setPath(e.target.value)}
              className="bg-transparent outline-none flex-1 placeholder:text-on-surface/10"
              placeholder="/volumes/disk/projects"
            />
          </div>
          <button className="flex items-center gap-2 px-5 py-3 border border-outline rounded-xl hover:bg-surface-bright transition-all text-xs font-bold text-on-surface/60 uppercase tracking-widest">
            <Edit className="w-3.5 h-3.5" />
            <span>Modify</span>
          </button>
        </div>
      </div>

      <div className="space-y-4">
        <h3 className="text-[10px] font-bold uppercase tracking-widest text-on-surface/30 px-2">Analysis Modules</h3>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {modules.map((mod) => (
            <motion.div
              key={mod.id}
              whileHover={{ y: -2 }}
              className={`bg-surface-container border border-outline rounded-2xl p-5 space-y-4 relative group transition-all
                ${mod.enabled ? 'border-primary/30 bg-primary/[0.02]' : 'opacity-60'}
              `}
            >
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-4">
                  <div className={`p-2.5 rounded-xl bg-surface-bright border border-outline ${mod.color} group-hover:scale-110 transition-transform shadow-inner`}>
                    <mod.icon className="w-5 h-5" />
                  </div>
                  <div className="space-y-0.5">
                    <h4 className="font-semibold text-sm text-on-surface">{mod.title}</h4>
                    <div className="flex items-center gap-2 bg-surface-dim px-1.5 py-0.5 rounded text-[9px] font-mono border border-outline/30 text-on-surface/40 w-fit">
                      {mod.path}
                    </div>
                  </div>
                </div>
                <div className={`w-10 h-5 rounded-full p-1 transition-colors cursor-pointer ${mod.enabled ? 'bg-primary' : 'bg-outline-variant'}`}>
                  <div className={`w-3 h-3 bg-white rounded-full transition-transform ${mod.enabled ? 'translate-x-5' : 'translate-x-0'}`} />
                </div>
              </div>
              <p className="text-xs text-on-surface/40 leading-relaxed min-h-[40px]">
                {mod.description}
              </p>
              {mod.badge && (
                <div className="flex justify-end">
                  <span className="text-[9px] bg-error/10 text-error px-2 py-0.5 rounded border border-error/20 font-bold uppercase tracking-tighter shadow-sm">
                    {mod.badge}
                  </span>
                </div>
              )}
            </motion.div>
          ))}
        </div>
      </div>

      <div className="pt-8 border-t border-outline flex justify-end">
        <button
          onClick={() => onInitiate(path)}
          className="group flex items-center gap-4 bg-on-surface text-surface px-10 py-4 rounded-xl font-bold text-xs uppercase tracking-[0.2em] hover:brightness-90 active:scale-[0.98] transition-all shadow-lg"
        >
          <Zap className="w-4 h-4 fill-current" />
          <span>Initiate Scan</span>
          <ChevronRight className="w-4 h-4 transition-transform group-hover:translate-x-1" />
        </button>
      </div>
    </div>
  );
}

function Monitor(props: any) {
  return (
    <svg
      {...props}
      xmlns="http://www.w3.org/2000/svg"
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <rect width="20" height="14" x="2" y="3" rx="2" />
      <line x1="8" x2="16" y1="21" y2="21" />
      <line x1="12" x2="12" y1="17" y2="21" />
    </svg>
  );
}
