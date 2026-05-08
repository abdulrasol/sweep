import React, { useState, useEffect } from 'react';
import { Search, CheckCircle2, AlertTriangle, Trash2, RefreshCw, EyeOff } from 'lucide-react';
import { motion } from 'motion/react';
import { invoke } from '../lib/tauriSimulation';

interface SystemInfo {
  os_name: string;
  os_version: string;
  cpu_usage: number;
  ram_total: number;
  ram_used: number;
  disk_total: number;
  disk_free: number;
}

interface ReviewViewProps {
  scanPath: string;
  selectedModules: string[];
  ignoredPaths: string[];
  autoPurge?: boolean;
  sysInfo: SystemInfo | null;
  onIgnore: (path: string) => void;
  onStartCleaning: (items: CleanupItem[]) => void;
}

export default function ReviewView({ scanPath, selectedModules, ignoredPaths, autoPurge = false, sysInfo, onIgnore, onStartCleaning }: ReviewViewProps) {
  const [items, setItems] = useState<CleanupItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState('');
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());

  useEffect(() => {
    const fetchData = async () => {
      setLoading(true);
      try {
        const data = await invoke<CleanupItem[]>('scan_environment', { 
          path: scanPath,
          modules: selectedModules,
          ignoredPaths: ignoredPaths
        });
        if (data) {
          setItems(data);
          const safeItems = data.filter(i => i.status === 'SAFE');
          const safeIds = new Set(safeItems.map(i => i.id));
          setSelectedIds(safeIds);

          // AUTO PURGE LOGIC
          if (autoPurge && safeItems.length > 0) {
            // Short delay to let user see that things were found before jumping
            setTimeout(() => {
               onStartCleaning(safeItems);
            }, 1500);
          }
        }
      } catch (err) {
        console.error("Scan failed:", err);
      } finally {
        setLoading(false);
      }
    };
    fetchData();
  }, [scanPath, selectedModules, ignoredPaths, autoPurge]);

  const toggleSelect = (id: string) => {
    const newSelected = new Set(selectedIds);
    if (newSelected.has(id)) newSelected.delete(id);
    else newSelected.add(id);
    setSelectedIds(newSelected);
  };

  const toggleAll = () => {
    if (selectedIds.size === items.length) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(items.map(i => i.id)));
    }
  };

  const handleIgnore = (path: string) => {
    onIgnore(path);
    setItems(prev => prev.filter(item => item.path !== path));
  };

  const totalSize = items.reduce((acc, item) => {
    if (selectedIds.has(item.id)) {
      const sizeVal = parseFloat(item.size);
      return acc + (item.size.includes('GB') ? sizeVal : sizeVal / 1024);
    }
    return acc;
  }, 0);

  const filteredItems = items.filter(item => 
    item.path.toLowerCase().includes(filter.toLowerCase()) || 
    item.type.toLowerCase().includes(filter.toLowerCase())
  );

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-full space-y-4">
        <RefreshCw className="w-8 h-8 text-primary animate-spin opacity-50" />
        <p className="font-mono text-[10px] uppercase tracking-[0.3em] text-on-surface/30">Targeting {selectedModules.length} Modules...</p>
      </div>
    );
  }

  return (
    <div className="max-w-6xl mx-auto py-8 px-6 space-y-6">
      {autoPurge && (
        <motion.div 
          initial={{ opacity: 0, y: -20 }}
          animate={{ opacity: 1, y: 0 }}
          className="bg-primary/10 border border-primary/20 rounded-xl p-4 flex items-center justify-between"
        >
          <div className="flex items-center gap-3 text-primary">
            <RefreshCw className="w-4 h-4 animate-spin" />
            <span className="text-[10px] font-bold uppercase tracking-widest">Automatic Purge Active: Bypassing Review Step</span>
          </div>
          <div className="text-[10px] font-mono text-primary/60">
            AUTO_INIT_READY
          </div>
        </motion.div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="md:col-span-2 bg-surface-container border border-outline rounded-xl p-8 flex flex-col justify-center space-y-6">
          <div className="space-y-2">
            <h2 className="text-2xl font-bold tracking-tight">Review Identified Junk</h2>
            <p className="text-sm text-on-surface/40 max-w-xl leading-relaxed">
              We found {items.length} items across the {selectedModules.length} active modules. Review the list below and select items to purge.
            </p>
          </div>
          <div className="flex gap-3">
            <button 
              onClick={() => onStartCleaning(items.filter(i => selectedIds.has(i.id)))}
              disabled={selectedIds.size === 0}
              className="px-6 py-2.5 bg-on-surface text-surface rounded-lg font-bold text-[10px] uppercase tracking-[0.1em] hover:brightness-90 active:scale-95 transition-all shadow-sm disabled:opacity-50"
            >
              Start Purge Sequence
            </button>
            <button 
              onClick={() => window.location.reload()}
              className="px-6 py-2.5 border border-outline rounded-lg font-bold text-[10px] uppercase tracking-[0.1em] hover:bg-surface-bright transition-all text-on-surface/60"
            >
              Adjust Scope
            </button>
          </div>
        </div>

        <div className="bg-surface-bright/50 border border-outline rounded-xl p-8 flex flex-col items-center justify-center text-center space-y-2 relative overflow-hidden group">
          <div className="absolute -top-4 -right-4 p-3 opacity-[0.03]">
            <Trash2 className="w-32 h-32 stroke-[1]" />
          </div>
          <span className="text-[10px] font-bold uppercase tracking-[0.2em] text-on-surface/30">Total Reclaimable</span>
          <div className="flex flex-col items-center">
            <div className="flex items-baseline gap-2 relative z-10">
              <motion.span 
                key={totalSize}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                className="text-6xl font-light font-sans tracking-tighter"
              >
                {totalSize.toFixed(1)}
              </motion.span>
              <span className="text-xs text-on-surface/40 font-mono">GB</span>
            </div>
            {sysInfo && (
              <div className="text-[10px] font-bold text-primary/60 uppercase tracking-widest mt-1">
                ≈ {((items.reduce((acc, i) => selectedIds.has(i.id) ? acc + i.size_bytes : acc, 0) / sysInfo.disk_total) * 100).toFixed(2)}% of Hard Drive
              </div>
            )}
          </div>
          <div className="w-12 h-0.5 bg-primary/30 rounded-full mt-2" />
        </div>
      </div>

      <div className="bg-surface-container border border-outline rounded-2xl overflow-hidden flex flex-col">
        <div className="p-4 border-b border-outline flex items-center justify-between bg-surface-bright/30">
          <div className="flex items-center gap-4">
             <label className="flex items-center gap-3 cursor-pointer group">
              <div 
                onClick={toggleAll}
                className={`w-4 h-4 rounded-md border transition-all flex items-center justify-center
                  ${selectedIds.size === items.length && items.length > 0 ? 'bg-primary border-primary' : 'border-outline-variant group-hover:border-primary'}
                `}
              >
                {selectedIds.size === items.length && items.length > 0 && <CheckCircle2 className="w-3 h-3 text-on-primary shrink-0" />}
              </div>
              <span className="text-[10px] font-bold uppercase tracking-widest text-on-surface/60">Select All Items</span>
            </label>
          </div>
          <div className="relative w-72">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-on-surface/30" />
            <input 
              placeholder="Filter by path or type..."
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              className="w-full bg-surface-dim border border-outline rounded-lg px-9 py-2 text-xs outline-none focus:border-primary/50 transition-all placeholder:text-on-surface/20"
            />
          </div>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full border-collapse">
            <thead>
              <tr className="border-b border-outline text-[10px] uppercase font-bold tracking-widest text-on-surface/30">
                <th className="w-12 p-4 text-center">
                  <RefreshCw className="w-3 h-3 mx-auto opacity-30" />
                </th>
                <th className="p-4 text-left">Path / Project</th>
                <th className="p-4 text-left">Type</th>
                <th className="p-4 text-right">Size</th>
                <th className="p-4 text-center">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-outline/50">
              {filteredItems.map((item) => (
                <motion.tr 
                  layout
                  key={item.id}
                  className={`hover:bg-surface-bright/50 transition-colors group ${selectedIds.has(item.id) ? 'bg-primary/[0.01]' : 'opacity-40'}`}
                >
                  <td className="p-4 text-center">
                    <div 
                      onClick={() => toggleSelect(item.id)}
                      className={`w-4 h-4 rounded-md border transition-all flex items-center justify-center mx-auto cursor-pointer
                        ${selectedIds.has(item.id) ? 'bg-primary border-primary' : 'border-outline-variant group-hover:border-primary'}
                      `}
                    >
                      {selectedIds.has(item.id) && <CheckCircle2 className="w-3 h-3 text-on-primary shrink-0" />}
                    </div>
                  </td>
                  <td className="p-4">
                    <div className="flex flex-col gap-2">
                      <div className="flex items-center gap-3">
                        {(item.status === 'DANGER' || item.status === 'WARNING' || item.status === 'UNSAFE') && (
                          <div className="flex items-center justify-center w-6 h-6 rounded-full bg-error/20 border border-error/30 animate-pulse">
                            <AlertTriangle className="w-3.5 h-3.5 text-error" />
                          </div>
                        )}
                        {item.status === 'REVIEW' && (
                          <div className="flex items-center justify-center w-6 h-6 rounded-full bg-yellow-500/20 border border-yellow-500/30">
                            <AlertTriangle className="w-3.5 h-3.5 text-yellow-500" />
                          </div>
                        )}
                        <span className="text-xs font-mono font-bold text-on-surface/90 truncate max-w-[400px]" title={item.path}>{item.path}</span>
                      </div>
                      <div className={`text-[11px] leading-relaxed p-2.5 rounded-xl border transition-all
                        ${item.status === 'SAFE' 
                          ? 'bg-surface-dim/50 border-outline/20 text-on-surface/40' 
                          : 'shadow-sm font-medium'}
                        ${(item.status === 'DANGER' || item.status === 'UNSAFE') 
                          ? 'bg-error/10 border-error/30 text-error' 
                          : ''}
                        ${item.status === 'REVIEW' 
                          ? 'bg-yellow-500/10 border-yellow-500/30 text-yellow-600' 
                          : ''}
                      `}>
                        {item.description}
                      </div>
                    </div>
                  </td>
                  <td className="p-4 text-[10px] font-bold text-on-surface/40 uppercase tracking-tighter">{item.type}</td>
                  <td className="p-4 text-right">
                    <div className="flex flex-col items-end">
                      <span className="text-xs font-mono font-medium text-on-surface/70">{item.size}</span>
                      {sysInfo && (
                        <span className="text-[9px] font-mono text-on-surface/30">
                          {((item.size_bytes / sysInfo.disk_total) * 100).toFixed(3)}%
                        </span>
                      )}
                    </div>
                  </td>
                  <td className="p-4 text-center">
                    <div className="flex items-center justify-center gap-2">
                       <span className={`text-[9px] font-bold px-2 py-0.5 rounded border uppercase tracking-[0.05em]
                        ${item.status === 'SAFE' ? 'bg-primary/10 text-primary border-primary/20' : 
                          item.status === 'REVIEW' ? 'bg-yellow-500/10 text-yellow-500 border-yellow-500/20' : 
                          'bg-error/10 text-error border-error/20'}
                      `}>
                        {item.status}
                      </span>
                      <button 
                        onClick={() => handleIgnore(item.path)}
                        className="p-1.5 hover:bg-surface-dim rounded-lg text-on-surface/30 hover:text-error transition-all group/ignore"
                        title="Ignore forever"
                      >
                        <EyeOff className="w-3.5 h-3.5 group-hover/ignore:scale-110" />
                      </button>
                    </div>
                  </td>
                </motion.tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
