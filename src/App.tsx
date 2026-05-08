import React, { useState, useEffect } from 'react';
import Sidebar from './components/Sidebar';
import Header from './components/Header';
import ScanView from './views/ScanView';
import ReviewView from './views/ReviewView';
import ExecutionView from './views/ExecutionView';
import SettingsView from './views/SettingsView';
import AboutView from './views/AboutView';
import { motion, AnimatePresence } from 'motion/react';
import { Activity, Zap, ShieldCheck } from 'lucide-react';
import { invoke } from './lib/tauriSimulation';

interface SystemInfo {
  os_name: string;
  os_version: string;
  cpu_usage: number;
  ram_total: number;
  ram_used: number;
  disk_total: number;
  disk_free: number;
}

type View = 'scan' | 'review' | 'execution' | 'settings' | 'about' | 'cleanup';

export default function App() {
  const [activeView, setActiveView] = useState<View>('scan');
  const [isCleaning, setIsCleaning] = useState(false);
  const [sysInfo, setSysInfo] = useState<SystemInfo | null>(null);

  const fetchSysInfo = async () => {
    try {
      const info = await invoke<SystemInfo>('get_system_info');
      setSysInfo(info);
    } catch (err) {
      console.error("Failed to fetch system info:", err);
    }
  };

  useEffect(() => {
    fetchSysInfo();
    const interval = setInterval(fetchSysInfo, 3000);
    return () => clearInterval(interval);
  }, []);
  
  const [scanPath, setScanPath] = useState(() => localStorage.getItem('sweep_path') || '');
  const [selectedModules, setSelectedModules] = useState<string[]>(() => {
    const saved = localStorage.getItem('sweep_modules');
    return saved ? JSON.parse(saved) : ['flutter', 'node', 'rust', 'xcode'];
  });
  const [ignoredPaths, setIgnoredPaths] = useState<string[]>(() => {
    const saved = localStorage.getItem('sweep_ignored');
    return saved ? JSON.parse(saved) : [];
  });
  
  const [selectedItems, setSelectedItems] = useState<CleanupItem[]>([]);
  const [accent, setAccent] = useState(() => localStorage.getItem('sweep_accent') || 'emerald');
  const [theme, setTheme] = useState<'light' | 'dark'>(() => (localStorage.getItem('sweep_theme') as any) || 'dark');
  
  const [verboseLogging, setVerboseLogging] = useState(() => localStorage.getItem('sweep_verbose') === 'true');
  const [autoPurge, setAutoPurge] = useState(() => localStorage.getItem('sweep_auto_purge') === 'true');

  useEffect(() => {
    localStorage.setItem('sweep_path', scanPath);
    localStorage.setItem('sweep_modules', JSON.stringify(selectedModules));
    localStorage.setItem('sweep_ignored', JSON.stringify(ignoredPaths));
    localStorage.setItem('sweep_accent', accent);
    localStorage.setItem('sweep_theme', theme);
    localStorage.setItem('sweep_verbose', String(verboseLogging));
    localStorage.setItem('sweep_auto_purge', String(autoPurge));
  }, [scanPath, selectedModules, ignoredPaths, accent, theme, verboseLogging, autoPurge]);

  useEffect(() => {
    document.documentElement.className = theme;
    const colors: Record<string, string> = {
      emerald: '#10b981',
      blue: '#3b82f6',
      violet: '#8b5cf6',
      rose: '#f43f5e',
      amber: '#f59e0b'
    };
    document.documentElement.style.setProperty('--color-primary', colors[accent]);
    
    const rgb = accent === 'emerald' ? '16, 185, 129' : 
                accent === 'blue' ? '59, 130, 246' :
                accent === 'violet' ? '139, 92, 246' :
                accent === 'rose' ? '244, 63, 94' : '245, 158, 11';
    document.documentElement.style.setProperty('--color-primary-rgb', rgb);
  }, [accent, theme]);

  const handleInitiateScan = (path: string, modules: string[]) => {
    setScanPath(path);
    setSelectedModules(modules);
    setActiveView('review');
  };

  const handleIgnorePath = (path: string) => {
    setIgnoredPaths(prev => [...new Set([...prev, path])]);
  };

  const renderViewContent = () => {
    if (isCleaning) {
      return (
        <ExecutionView 
          items={selectedItems} 
          verbose={verboseLogging}
          onComplete={() => {
            setIsCleaning(false);
            setActiveView('scan');
          }} 
        />
      );
    }

    switch (activeView) {
      case 'scan':
        return (
          <ScanView 
            initialPath={scanPath}
            initialModules={selectedModules}
            onInitiate={handleInitiateScan} 
          />
        );
      case 'review':
        return (
          <ReviewView 
            scanPath={scanPath}
            selectedModules={selectedModules}
            ignoredPaths={ignoredPaths}
            autoPurge={autoPurge}
            sysInfo={sysInfo}
            onIgnore={handleIgnorePath}
            onStartCleaning={(items: CleanupItem[]) => {
              setSelectedItems(items);
              setIsCleaning(true);
            }} 
          />
        );
      case 'cleanup':
        return (
          <div className="h-full flex flex-col items-center justify-center space-y-6 text-center px-6">
            <div className="w-20 h-20 rounded-3xl bg-primary/10 border border-primary/20 flex items-center justify-center shadow-inner">
               <Activity className="w-10 h-10 text-primary animate-pulse" />
            </div>
            <div className="space-y-2">
              <h2 className="text-xl font-bold tracking-tight text-on-surface">Purge Engine: Standing By</h2>
              <p className="text-xs text-on-surface/40 max-w-sm leading-relaxed">
                The high-performance cleanup sequence is armed and ready. Initiate a scan in <span className="text-primary font-bold">Target Scope</span> to identify artifacts for removal.
              </p>
            </div>
            <button 
              onClick={() => setActiveView('scan')}
              className="flex items-center gap-3 px-8 py-3 bg-on-surface text-surface rounded-xl font-bold text-[10px] uppercase tracking-[0.2em] hover:brightness-90 transition-all active:scale-95"
            >
              <Zap className="w-3.5 h-3.5 fill-current" />
              Go to Target Scope
            </button>
            <div className="pt-8 flex items-center gap-6 opacity-20">
              <div className="flex items-center gap-2">
                <ShieldCheck className="w-4 h-4" />
                <span className="text-[10px] font-mono uppercase">Root Authorized</span>
              </div>
              <div className="flex items-center gap-2">
                <div className="w-1.5 h-1.5 rounded-full bg-primary" />
                <span className="text-[10px] font-mono uppercase">Engine Warm</span>
              </div>
            </div>
          </div>
        );
      case 'settings':
        return (
          <SettingsView 
            accent={accent}
            onAccentChange={setAccent}
            theme={theme}
            onThemeChange={setTheme}
            verbose={verboseLogging}
            onVerboseChange={setVerboseLogging}
            autoPurge={autoPurge}
            onAutoPurgeChange={setAutoPurge}
          />
        );
      case 'about':
        return <AboutView />;
      default:
        return <ScanView initialPath={scanPath} initialModules={selectedModules} onInitiate={handleInitiateScan} />;
    }
  };

  return (
    <div className="flex h-screen bg-surface text-on-surface selection:bg-primary/20 overflow-hidden font-sans">
      <Sidebar activeView={activeView} onViewChange={(v) => setActiveView(v as View)} sysInfo={sysInfo} />
      <div className="flex-1 flex flex-col overflow-hidden relative">
        <Header 
          accent={accent} 
          setAccent={setAccent} 
          theme={theme} 
          setTheme={setTheme}
          ignoredCount={ignoredPaths.length}
          onClearIgnores={() => setIgnoredPaths([])}
        />
        <main className="flex-1 overflow-hidden bg-surface-dim/30 relative">
          <AnimatePresence mode="wait">
            <motion.div 
              key={activeView + (isCleaning ? '-clean' : '')}
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              transition={{ duration: 0.2 }}
              className="h-full overflow-y-auto"
            >
              {renderViewContent()}
            </motion.div>
          </AnimatePresence>
        </main>
      </div>
    </div>
  );
}
