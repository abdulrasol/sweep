import React, { useState, useRef, useEffect } from 'react';
import { Terminal, Cpu, Activity, CheckCircle2, X, Copy, Trash2 } from 'lucide-react';
import { motion } from 'motion/react';
import { invoke } from '../lib/tauriSimulation';

interface ExecutionViewProps {
  items: CleanupItem[];
  verbose?: boolean;
  onComplete: () => void;
}

export default function ExecutionView({ items, verbose = false, onComplete }: ExecutionViewProps) {
  const [progress, setProgress] = useState(0);
  const [logs, setLogs] = useState<string[]>([]);
  const [processedCount, setProcessedCount] = useState(0);
  const [freedSize, setFreedSize] = useState(0);
  const logEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [logs]);

  const addLog = (msg: string) => {
    setLogs(prev => [...prev, `[${new Date().toLocaleTimeString()}] ${msg}`]);
  };

  useEffect(() => {
    const runCleanup = async () => {
      addLog(`Initializing purge sequence for ${items.length} targets...`);
      if (verbose) {
        addLog("SYSTEM: Running with high-verbosity orchestration.");
        addLog(`DEBUG: Tauri bridge established. OS detected: ${navigator.platform}`);
        addLog(`DEBUG: Buffer allocated for ${items.length} file descriptors.`);
      }
      
      let totalFreed = 0;

      for (let i = 0; i < items.length; i++) {
        const item = items[i];
        
        if (verbose) {
          addLog(`SYSCALL: Preparing FS-unlink for path ${item.path}`);
          addLog(`DEBUG: Target type detected as ${item.file_type || item.type} (${item.category || 'Unknown'})`);
        }

        addLog(`Purging: ${item.path}...`);
        
        try {
          await invoke('cleanup_item', { item });
          const sizeVal = parseFloat(item.size);
          const sizeGB = item.size.includes('GB') ? sizeVal : sizeVal / 1024;
          
          totalFreed += sizeGB;
          setFreedSize(totalFreed);
          
          if (verbose) {
            addLog(`SYSCALL: Unlink SUCCESS. Disk block reclaimed.`);
          }
          addLog(`SUCCESS: Reclaimed ${item.size}`);
        } catch (err) {
          if (verbose) addLog(`DEBUG: Error details: ${JSON.stringify(err)}`);
          addLog(`ERROR: Failed to purge ${item.path}`);
        }
        
        setProcessedCount(i + 1);
        setProgress(((i + 1) / items.length) * 100);
      }
      
      addLog(`Cleanup sequence complete. Total space reclaimed: ${totalFreed.toFixed(1)} GB.`);
      if (verbose) {
        addLog("SYSTEM: Flushing IO buffers.");
        addLog("DEBUG: Process detached. Returning control to main thread.");
      }
    };

    if (items.length > 0) {
      runCleanup();
    }
  }, [items, verbose]);

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
              Processed {processedCount} / {items.length}
            </div>
          </div>
        </div>

        <div className="relative h-2 bg-surface-bright rounded-full overflow-hidden">
          <motion.div 
            className="absolute top-0 left-0 h-full bg-primary shadow-[0_0_10px_rgba(var(--color-primary-rgb),0.3)]"
            initial={{ width: 0 }}
            animate={{ width: `${progress}%` }}
          />
        </div>

        <div className="flex gap-4 relative z-10">
          <div className="flex items-center gap-2 bg-surface-dim border border-outline px-3 py-1.5 rounded-lg shadow-sm">
            <div className="w-1.5 h-1.5 rounded-full bg-primary animate-pulse" />
            <span className="text-[9px] font-bold uppercase tracking-widest text-on-surface/50">Active Execution</span>
          </div>
          <div className="flex items-center gap-2 bg-surface-dim border border-outline px-3 py-1.5 rounded-lg shadow-sm">
            <Cpu className="w-3 h-3 text-on-surface/30" />
            <span className="text-[9px] font-bold uppercase tracking-widest text-on-surface/50">RAM: Stable</span>
          </div>
          <div className="flex items-center gap-2 bg-surface-dim border border-outline px-3 py-1.5 rounded-lg shadow-sm">
            <Trash2 className="w-3 h-3 text-primary opacity-60" />
            <span className="text-[9px] font-bold uppercase tracking-widest text-primary/80">Purged: {freedSize.toFixed(1)} GB</span>
          </div>
        </div>
      </div>

      {/* Console Output */}
      <div className="flex-1 flex flex-col bg-[#050505] border border-outline rounded-2xl overflow-hidden min-h-[300px] shadow-2xl terminal-glow">
        <div className="bg-surface-bright/50 px-5 py-3 border-b border-outline flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Terminal className="w-3.5 h-3.5 text-primary opacity-70" />
            <span className="text-[10px] font-mono font-bold uppercase tracking-[0.2em] text-on-surface/40">system.orchestrator.log</span>
          </div>
          <div className="flex gap-3 text-[10px] font-mono text-primary/30 uppercase tracking-widest">
            {verbose ? "VERBOSE_ON" : "STANDARD_LOG"}
          </div>
        </div>
        <div className="p-5 font-mono text-[11px] leading-relaxed overflow-y-auto flex-1 space-y-1.5 scrollbar-thin scrollbar-thumb-outline scrollbar-track-transparent">
          {logs.map((log, i) => {
             const isSuccess = log.includes('SUCCESS');
             const isError = log.includes('ERROR');
             const isDebug = log.includes('DEBUG') || log.includes('SYSCALL');
             return (
               <div key={i} className="flex gap-3 animate-in fade-in slide-in-from-left-2 transition-all">
                 <span className={`${isSuccess ? 'text-primary' : isError ? 'text-error' : isDebug ? 'text-primary/30' : 'text-primary/60'} font-bold shrink-0 uppercase tracking-tighter w-16`}>
                   {isSuccess ? 'SUCCESS' : isError ? 'ERROR' : isDebug ? (log.includes('SYSCALL') ? 'SYSCALL' : 'DEBUG') : 'INFO'}
                 </span>
                 <span className={`font-light ${isDebug ? 'text-on-surface/20' : 'text-on-surface/60'}`}>
                    {log.includes('] ') ? log.split('] ')[1] : log}
                 </span>
               </div>
             );
           })}
          <div ref={logEndRef} />
        </div>
      </div>

      <div className="pt-4 flex justify-end">
        <button
          onClick={onComplete}
          className="flex items-center gap-2 px-8 py-3 border border-outline text-on-surface/40 rounded-xl hover:bg-surface-bright hover:text-on-surface transition-all font-bold text-[10px] uppercase tracking-widest active:scale-95"
        >
          <CheckCircle2 className="w-4 h-4" />
          Finish & Return
        </button>
      </div>
    </div>
  );
}
