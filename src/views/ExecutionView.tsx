import { useState, useRef, useEffect } from 'react';
import { Terminal, Cpu, Database, Save, Activity, CheckCircle2, AlertCircle, X, Copy, Trash2 } from 'lucide-react';
import { motion, AnimatePresence } from 'motion/react';
import { useScanningSimulation } from '../lib/tauriSimulation';

interface ExecutionViewProps {
  onAbort: () => void;
}

export default function ExecutionView({ onAbort }: ExecutionViewProps) {
  const { progress, logs, freed } = useScanningSimulation(true);
  const logEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [logs]);

  const targets = [
    { path: '/var/www/project-alpha/node_modules', progress: Math.min(progress * 1.5, 45), size: '-1.2GB', status: 'Cleaning' },
    { path: '/users/dev/workspace/frontend-v2/.next/cache', progress: Math.min(progress * 1.2, 82), size: '-850MB', status: 'Cleaning' },
    { path: '/users/dev/docker/volumes/temp-data', progress: 100, size: '-2.1GB', status: 'Done' },
    { path: '/Library/Caches/com.apple.dt.Xcode', progress: 0, size: 'Est. 5GB', status: 'Queued' },
  ];

  return (
    <div className="max-w-6xl mx-auto py-8 px-6 space-y-6 flex flex-col h-full overflow-hidden">
      {/* Progress Header */}
      <div className="bg-surface-container border border-outline rounded-2xl p-8 space-y-8 relative overflow-hidden">
        <div className="absolute top-0 right-0 w-64 h-64 bg-primary/5 rounded-full blur-[80px] -mr-32 -mt-32 pointer-events-none" />
        <div className="flex items-start justify-between relative z-10">
          <div className="space-y-2">
            <h2 className="text-2xl font-bold tracking-tight">Execution Sequence Active</h2>
            <p className="text-sm text-on-surface/40 max-w-md leading-relaxed">
              Purging selected node modules, build artifacts, and orphaned dependencies from system storage.
            </p>
          </div>
          <div className="text-right">
            <div className="text-6xl font-light text-primary font-sans tracking-tighter">
              {Math.round(progress)}%
            </div>
            <div className="text-[10px] font-bold uppercase tracking-[0.2em] text-on-surface/30 mt-1">
              Est. Remaining: 42s
            </div>
          </div>
        </div>

        <div className="relative h-2 bg-surface-bright rounded-full overflow-hidden">
          <motion.div 
            className="absolute top-0 left-0 h-full bg-primary shadow-[0_0_10px_rgba(16,185,129,0.3)]"
            initial={{ width: 0 }}
            animate={{ width: `${progress}%` }}
          />
        </div>

        <div className="flex gap-4 relative z-10">
          <div className="flex items-center gap-2 bg-surface-dim border border-outline px-3 py-1.5 rounded-lg shadow-sm">
            <div className="w-1.5 h-1.5 rounded-full bg-primary animate-pulse" />
            <span className="text-[9px] font-bold uppercase tracking-widest text-on-surface/50">4 Active Threads</span>
          </div>
          <div className="flex items-center gap-2 bg-surface-dim border border-outline px-3 py-1.5 rounded-lg shadow-sm">
            <Cpu className="w-3 h-3 text-on-surface/30" />
            <span className="text-[9px] font-bold uppercase tracking-widest text-on-surface/50">RAM: 1.2 GB</span>
          </div>
          <div className="flex items-center gap-2 bg-surface-dim border border-outline px-3 py-1.5 rounded-lg shadow-sm">
            <Trash2 className="w-3 h-3 text-primary opacity-60" />
            <span className="text-[9px] font-bold uppercase tracking-widest text-primary/80">Purged: {freed.toFixed(1)} GB</span>
          </div>
        </div>
      </div>

      {/* Active Targets */}
      <div className="space-y-4">
        <h3 className="text-[10px] font-bold uppercase tracking-widest text-on-surface/30 px-2">Purge Targets</h3>
        <div className="grid grid-cols-1 gap-2">
          {targets.map((target, idx) => (
            <div key={idx} className="bg-surface-container border border-outline rounded-xl px-5 py-4 flex items-center gap-4 group hover:border-primary/20 transition-all">
              <div className="p-2 bg-surface-bright rounded-lg border border-outline shadow-inner">
                {target.status === 'Cleaning' ? (
                  <Activity className="w-4 h-4 text-primary animate-pulse" />
                ) : target.status === 'Done' ? (
                  <CheckCircle2 className="w-4 h-4 text-primary" />
                ) : (
                  <Activity className="w-4 h-4 text-on-surface/20" />
                )}
              </div>
              <div className="flex-1 flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-mono font-bold text-on-surface/80 truncate max-w-md">{target.path}</span>
                  <div className="flex items-center gap-4">
                    <span className="text-[9px] font-bold uppercase tracking-widest text-on-surface/30">{target.status} • {Math.round(target.progress)}%</span>
                    <span className={`text-xs font-mono font-bold ${target.status === 'Done' ? 'text-primary' : 'text-on-surface/30'}`}>
                      {target.size}
                    </span>
                  </div>
                </div>
                <div className="h-1 bg-surface-bright rounded-full overflow-hidden">
                  <motion.div 
                    initial={{ width: 0 }}
                    animate={{ width: `${target.progress}%` }}
                    className={`h-full ${target.status === 'Done' ? 'bg-primary' : 'bg-primary/40'}`}
                  />
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Console Output */}
      <div className="flex-1 flex flex-col bg-[#050505] border border-outline rounded-2xl overflow-hidden min-h-[180px] shadow-2xl terminal-glow">
        <div className="bg-surface-bright/50 px-5 py-3 border-b border-outline flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Terminal className="w-3.5 h-3.5 text-primary opacity-70" />
            <span className="text-[10px] font-mono font-bold uppercase tracking-[0.2em] text-on-surface/40">system.orchestrator.log</span>
          </div>
          <div className="flex gap-3">
            {[Copy, Trash2].map((Icon, i) => (
              <button key={i} className="p-1 text-on-surface/20 hover:text-primary transition-colors">
                <Icon className="w-3.5 h-3.5" />
              </button>
            ))}
          </div>
        </div>
        <div className="p-5 font-mono text-[11px] leading-relaxed overflow-y-auto flex-1 space-y-1.5 scrollbar-thin scrollbar-thumb-outline scrollbar-track-transparent">
          {logs.map((log, i) => {
             const isSuccess = log.includes('SUCCESS');
             const isWarn = log.includes('WARN');
             return (
               <div key={i} className="flex gap-3 animate-in fade-in slide-in-from-left-2 transition-all">
                 <span className={`${isSuccess ? 'text-primary' : isWarn ? 'text-yellow-500' : 'text-primary/60'} font-bold shrink-0 uppercase tracking-tighter w-16`}>
                   {isSuccess ? 'SUCCESS' : isWarn ? 'WARNING' : 'INFO'}
                 </span>
                 <span className="text-on-surface/60 font-light">{log.split('] ')[1]}</span>
               </div>
             );
          })}
          <div ref={logEndRef} />
        </div>
      </div>

      <div className="pt-4 flex justify-end">
        <button
          onClick={onAbort}
          className="flex items-center gap-2 px-8 py-3 border border-error/30 text-error/70 rounded-xl hover:bg-error/5 hover:text-error transition-all font-bold text-[10px] uppercase tracking-widest active:scale-95"
        >
          <X className="w-4 h-4" />
          Abort Purge
        </button>
      </div>
    </div>
  );
}
