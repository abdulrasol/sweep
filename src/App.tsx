import { useState } from 'react';
import Sidebar from './components/Sidebar';
import Header from './components/Header';
import ScanView from './views/ScanView';
import ReviewView from './views/ReviewView';
import ExecutionView from './views/ExecutionView';
import SettingsView from './views/SettingsView';
import AboutView from './views/AboutView';
import { AnimatePresence, motion } from 'motion/react';

type View = 'scan' | 'review' | 'cleanup' | 'analytics' | 'settings' | 'about';

export default function App() {
  const [activeView, setActiveView] = useState<View>('scan');
  const [isCleaning, setIsCleaning] = useState(false);

  const handleInitiateScan = (path: string) => {
    console.log(`Scanning path: ${path}`);
    setActiveView('review');
  };

  const handleStartCleaning = (items: any[]) => {
    console.log(`Starting cleanup for ${items.length} items`);
    setIsCleaning(true);
    setActiveView('cleanup');
  };

  const handleAbort = () => {
    setIsCleaning(false);
    setActiveView('scan');
  };

  const renderView = () => {
    switch (activeView) {
      case 'scan':
        return <ScanView onInitiate={handleInitiateScan} />;
      case 'review':
        return <ReviewView onStartCleaning={handleStartCleaning} />;
      case 'cleanup':
        return <ExecutionView onAbort={handleAbort} />;
      case 'settings':
        return <SettingsView />;
      case 'about':
        return <AboutView />;
      default:
        return (
          <div className="flex flex-col items-center justify-center h-full text-on-surface/20">
            <motion.div
              initial={{ rotate: 0 }}
              animate={{ rotate: 360 }}
              transition={{ duration: 20, repeat: Infinity, ease: "linear" }}
            >
              <div className="w-64 h-64 border-2 border-dashed border-current rounded-full" />
            </motion.div>
            <p className="mt-8 font-mono text-xs uppercase tracking-[0.4em]">Module coming soon</p>
          </div>
        );
    }
  };

  const getHeaderTitle = () => {
    switch (activeView) {
      case 'scan': return 'System Scan';
      case 'review': return 'Review Targets';
      case 'cleanup': return 'Purge Sequence';
      case 'settings': return 'System Settings';
      case 'about': return 'About DevClean';
      default: return 'DevUtil';
    }
  };

  return (
    <div className="flex h-screen w-full bg-surface-dim selection:bg-primary/20 overflow-hidden font-sans">
      <Sidebar activeView={activeView} onViewChange={(v) => setActiveView(v as View)} />
      
      <main className="flex-1 flex flex-col relative overflow-hidden bg-surface">
        <Header title={getHeaderTitle()} />
        
        <div className="flex-1 overflow-y-auto scroll-smooth custom-scrollbar">
          <AnimatePresence mode="wait">
            <motion.div
              key={activeView}
              initial={{ opacity: 0, scale: 0.98 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 1.02 }}
              transition={{ duration: 0.25, ease: [0.22, 1, 0.36, 1] }}
              className="h-full"
            >
              {renderView()}
            </motion.div>
          </AnimatePresence>
        </div>

        {/* Bottom Rail */}
        <footer className="h-8 bg-surface-container border-t border-outline flex items-center px-4 justify-between shrink-0 z-10">
          <div className="flex items-center gap-4 text-[10px] font-mono text-on-surface/40 uppercase tracking-widest">
            <span className="flex items-center gap-2">
              <span className="w-2 h-2 bg-secondary rounded-sm shadow-[0_0_5px_rgba(59,130,246,0.3)]"></span> 
              Main Thread
            </span>
            <div className="w-px h-3 bg-outline"></div>
            <span>0 Errors</span>
            <span>0 Warnings</span>
          </div>
          <div className="text-[9px] font-mono text-on-surface/30 uppercase tracking-[0.2em]">
            UTF-8 | LF | Rust Stable v1.75
          </div>
        </footer>

        {/* Ambient background effects */}
        <div className="absolute top-0 right-0 w-[600px] h-[600px] bg-primary/[0.03] rounded-full blur-[120px] pointer-events-none -z-10 animate-pulse" />
        <div className="absolute bottom-0 left-0 w-[400px] h-[400px] bg-secondary/[0.02] rounded-full blur-[100px] pointer-events-none -z-10" />
      </main>
    </div>
  );
}
