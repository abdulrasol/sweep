import { useState, useEffect } from 'react';
import { Search, Info, CheckCircle2, AlertTriangle, Trash2, RefreshCw } from 'lucide-react';
import { motion, AnimatePresence } from 'motion/react';
import { invoke } from '../lib/tauriSimulation';

interface ReviewViewProps {
  onStartCleaning: (items: any[]) => void;
}

export default function ReviewView({ onStartCleaning }: ReviewViewProps) {
  const [items, setItems] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState('');
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());

  useEffect(() => {
    const fetchData = async () => {
      const data = await invoke('scan_environment');
      setItems(data);
      setSelectedIds(new Set(data.filter((i: any) => i.status === 'SAFE').map((i: any) => i.id)));
      setLoading(false);
    };
    fetchData();
  }, []);

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

  return (
    <div className="max-w-6xl mx-auto py-8 px-6 space-y-6">
      {/* Top Banner Stats */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="md:col-span-2 bg-surface-container border border-outline rounded-xl p-8 flex flex-col justify-center space-y-6">
          <div className="space-y-2">
            <h2 className="text-2xl font-bold tracking-tight">Review Identified Junk</h2>
            <p className="text-sm text-on-surface/40 max-w-xl leading-relaxed">
              We found multiple temporary files, build artifacts, and stale dependencies across your projects. Review the list below and select items to purge.
            </p>
          </div>
          <div className="flex gap-3">
            <button 
              onClick={() => onStartCleaning(items.filter(i => selectedIds.has(i.id)))}
              className="px-6 py-2.5 bg-on-surface text-surface rounded-lg font-bold text-[10px] uppercase tracking-[0.1em] hover:brightness-90 active:scale-95 transition-all shadow-sm"
            >
              Start Purge Sequence
            </button>
            <button className="px-6 py-2.5 border border-outline rounded-lg font-bold text-[10px] uppercase tracking-[0.1em] hover:bg-surface-bright transition-all text-on-surface/60">
              Re-Scan Disk
            </button>
          </div>
        </div>

        <div className="bg-surface-bright/50 border border-outline rounded-xl p-8 flex flex-col items-center justify-center text-center space-y-2 relative overflow-hidden group">
          <div className="absolute -top-4 -right-4 p-3 opacity-[0.03]">
            <Trash2 className="w-32 h-32 stroke-[1]" />
          </div>
          <span className="text-[10px] font-bold uppercase tracking-[0.2em] text-on-surface/30">Total Reclaimable</span>
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
          <div className="w-12 h-0.5 bg-primary/30 rounded-full mt-2" />
        </div>
      </div>

      {/* Table Section */}
      <div className="bg-surface-container border border-outline rounded-2xl overflow-hidden flex flex-col">
        <div className="p-4 border-b border-outline flex items-center justify-between bg-surface-bright/30">
          <div className="flex items-center gap-4">
             <label className="flex items-center gap-3 cursor-pointer group">
              <div 
                onClick={toggleAll}
                className={`w-4 h-4 rounded-md border transition-all flex items-center justify-center
                  ${selectedIds.size === items.length ? 'bg-primary border-primary' : 'border-outline-variant group-hover:border-primary'}
                `}
              >
                {selectedIds.size === items.length && <CheckCircle2 className="w-3 h-3 text-on-primary shrink-0" />}
              </div>
              <span className="text-[10px] font-bold uppercase tracking-widest text-on-surface/60">Select All Safe</span>
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
                <th className="p-4 text-center">Status</th>
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
                    <div className="flex items-center gap-3">
                      {item.status === 'UNSAFE' && <AlertTriangle className="w-3.5 h-3.5 text-error opacity-70" />}
                      <span className="text-xs font-mono text-on-surface/80">{item.path}</span>
                    </div>
                  </td>
                  <td className="p-4 text-[10px] font-bold text-on-surface/40 uppercase tracking-tighter">{item.type}</td>
                  <td className="p-4 text-right text-xs font-mono font-medium text-on-surface/70">{item.size}</td>
                  <td className="p-4 text-center">
                    <span className={`text-[9px] font-bold px-2 py-0.5 rounded border uppercase tracking-[0.05em]
                      ${item.status === 'SAFE' ? 'bg-primary/10 text-primary border-primary/20' : 
                        item.status === 'REVIEW' ? 'bg-yellow-500/10 text-yellow-500 border-yellow-500/20' : 
                        'bg-error/10 text-error border-error/20'}
                    `}>
                      {item.status}
                    </span>
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
