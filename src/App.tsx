import { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { ListChecks } from 'lucide-react';
import Sidebar from './components/Sidebar';
import Header from './components/Header';
import ScanView from './views/ScanView';
import ReviewView from './views/ReviewView';
import ExecutionView from './views/ExecutionView';
import SettingsView from './views/SettingsView';
import AboutView from './views/AboutView';
import { invoke } from './lib/tauri';
import { accentById } from './lib/accents';

const readJson = <T,>(key: string, fallback: T): T => {
  try {
    const saved = localStorage.getItem(key);
    return saved ? (JSON.parse(saved) as T) : fallback;
  } catch {
    return fallback;
  }
};

export default function App() {
  const [activeView, setActiveView] = useState<View>('scan');
  const [sysInfo, setSysInfo] = useState<SystemInfo | null>(null);
  const [version, setVersion] = useState('');

  // Items handed to the Clean step. Null when nothing has been started.
  const [cleanRun, setCleanRun] = useState<CleanupItem[] | null>(null);
  const [cleaning, setCleaning] = useState(false);

  const [scanPath, setScanPath] = useState(() => localStorage.getItem('sweep_path') || '');
  const [selectedModules, setSelectedModules] = useState<string[]>(() =>
    readJson('sweep_modules', ['flutter', 'node', 'rust', 'xcode']),
  );
  const [ignoredPaths, setIgnoredPaths] = useState<string[]>(() => readJson('sweep_ignored', []));
  const [accent, setAccent] = useState(() => localStorage.getItem('sweep_accent') || 'emerald');
  const [theme, setTheme] = useState<'light' | 'dark'>(() =>
    localStorage.getItem('sweep_theme') === 'light' ? 'light' : 'dark',
  );

  useEffect(() => {
    const fetchSysInfo = () =>
      invoke<SystemInfo>('get_system_info').then(setSysInfo).catch(() => undefined);
    fetchSysInfo();
    const interval = setInterval(fetchSysInfo, 3000);
    invoke<string>('get_app_version').then(setVersion).catch(() => undefined);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    localStorage.setItem('sweep_path', scanPath);
    localStorage.setItem('sweep_modules', JSON.stringify(selectedModules));
    localStorage.setItem('sweep_ignored', JSON.stringify(ignoredPaths));
    localStorage.setItem('sweep_accent', accent);
    localStorage.setItem('sweep_theme', theme);
    localStorage.removeItem('sweep_auto_purge');
    localStorage.removeItem('sweep_verbose');
  }, [scanPath, selectedModules, ignoredPaths, accent, theme]);

  useEffect(() => {
    const root = document.documentElement;
    const a = accentById(accent);
    root.className = theme;
    root.style.setProperty('--color-primary', a.hex);
    root.style.setProperty('--color-primary-rgb', a.rgb);
  }, [accent, theme]);

  const navigate = (view: View) => {
    if (cleaning) return; // Stay on the Clean step until the run stops.
    if (view !== 'cleanup') setCleanRun(null);
    setActiveView(view);
  };

  const startScan = (path: string, modules: string[]) => {
    setScanPath(path);
    setSelectedModules(modules);
    setActiveView('review');
  };

  const startCleaning = (items: CleanupItem[]) => {
    setCleanRun(items);
    setActiveView('cleanup');
  };

  const renderView = () => {
    switch (activeView) {
      case 'scan':
        return <ScanView initialPath={scanPath} initialModules={selectedModules} onScan={startScan} />;
      case 'review':
        return (
          <ReviewView
            scanPath={scanPath}
            selectedModules={selectedModules}
            ignoredPaths={ignoredPaths}
            onIgnore={(path) => setIgnoredPaths(prev => [...new Set([...prev, path])])}
            onStartCleaning={startCleaning}
            onChangeScope={() => setActiveView('scan')}
          />
        );
      case 'cleanup':
        if (cleanRun) {
          return (
            <ExecutionView
              items={cleanRun}
              onRunningChange={setCleaning}
              onScanAgain={() => navigate('review')}
              onChangeScope={() => navigate('scan')}
            />
          );
        }
        return (
          <div className="h-full flex flex-col items-center justify-center gap-4 text-center px-6">
            <ListChecks className="w-8 h-8 text-on-surface/30" strokeWidth={1.5} />
            <div className="space-y-1.5">
              <h2 className="text-lg font-semibold">Nothing is being cleaned</h2>
              <p className="text-sm text-on-surface/60 max-w-sm">
                Scan your projects, pick what to remove in Review, and the cleanup runs here.
              </p>
            </div>
            <button
              onClick={() => navigate('review')}
              className="mt-2 px-4 py-2 rounded-lg bg-on-surface text-surface text-sm font-medium hover:opacity-90 transition-opacity"
            >
              Go to Review
            </button>
          </div>
        );
      case 'settings':
        return (
          <SettingsView
            accent={accent}
            onAccentChange={setAccent}
            theme={theme}
            onThemeChange={setTheme}
            ignoredPaths={ignoredPaths}
            onUnhide={(path) => setIgnoredPaths(prev => prev.filter(p => p !== path))}
            onUnhideAll={() => setIgnoredPaths([])}
          />
        );
      case 'about':
        return <AboutView version={version} />;
    }
  };

  return (
    <div className="flex h-screen bg-surface text-on-surface overflow-hidden">
      <Sidebar activeView={activeView} onNavigate={navigate} locked={cleaning} sysInfo={sysInfo} />
      <div className="flex-1 flex flex-col overflow-hidden min-w-0">
        <Header
          scanPath={scanPath}
          accent={accent}
          setAccent={setAccent}
          theme={theme}
          setTheme={setTheme}
          hiddenCount={ignoredPaths.length}
          onShowHidden={() => navigate('settings')}
          version={version}
        />
        <main className="flex-1 overflow-hidden bg-surface-dim relative">
          <AnimatePresence mode="wait">
            <motion.div
              key={activeView}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.12 }}
              className="h-full overflow-y-auto"
            >
              {renderView()}
            </motion.div>
          </AnimatePresence>
        </main>
      </div>
    </div>
  );
}
