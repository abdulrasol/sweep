import { Terminal, Settings, Share2, Monitor, Cpu, Moon, Sun, Palette, Trash2 } from 'lucide-react';
import { motion, AnimatePresence } from 'motion/react';

interface HeaderProps {
  accent: string;
  setAccent: (accent: string) => void;
  theme: 'light' | 'dark';
  setTheme: (theme: 'light' | 'dark') => void;
  ignoredCount: number;
  onClearIgnores: () => void;
}

export default function Header({ 
  accent, 
  setAccent, 
  theme, 
  setTheme, 
  ignoredCount, 
  onClearIgnores 
}: HeaderProps) {
  
  const colors = [
    { id: 'emerald', hex: '#10b981', class: 'bg-[#10b981]' },
    { id: 'blue', hex: '#3b82f6', class: 'bg-[#3b82f6]' },
    { id: 'violet', hex: '#8b5cf6', class: 'bg-[#8b5cf6]' },
    { id: 'rose', hex: '#f43f5e', class: 'bg-[#f43f5e]' },
    { id: 'amber', hex: '#f59e0b', class: 'bg-[#f59e0b]' },
  ];

  return (
    <header className="h-16 border-b border-outline bg-surface-container/80 backdrop-blur-md flex items-center justify-between px-6 shrink-0 z-10 w-full transition-colors duration-300">
      <div className="flex items-center gap-4">
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-2 bg-surface-bright border border-outline px-3 py-1 rounded-lg">
            <Cpu className="w-3.5 h-3.5 text-primary opacity-60" />
            <span className="text-[10px] font-mono font-bold uppercase tracking-widest text-on-surface/40">Sweep.core</span>
          </div>
        </div>
      </div>

      <div className="flex items-center gap-4">
        {/* Ignored Count Badge */}
        {ignoredCount > 0 && (
          <motion.button 
            initial={{ scale: 0 }}
            animate={{ scale: 1 }}
            onClick={onClearIgnores}
            className="flex items-center gap-2 px-3 py-1 bg-error/10 border border-error/20 rounded-full group hover:bg-error/20 transition-all"
            title="Clear ignore list"
          >
            <Trash2 className="w-3 h-3 text-error" />
            <span className="text-[9px] text-error font-bold uppercase tracking-tighter">{ignoredCount} Ignored</span>
          </motion.button>
        )}

        {/* Accent Selector */}
        <div className="flex items-center gap-1.5 px-2 py-1 bg-surface-bright/50 border border-outline rounded-xl">
          {colors.map((c) => (
            <button
              key={c.id}
              onClick={() => setAccent(c.id)}
              className={`w-4 h-4 rounded-full transition-all hover:scale-125 ${c.class} ${accent === c.id ? 'ring-2 ring-on-surface ring-offset-2 ring-offset-surface scale-110' : 'opacity-40 hover:opacity-100'}`}
            />
          ))}
        </div>

        <div className="w-px h-6 bg-outline mx-1 opacity-50"></div>

        {/* Theme Toggle */}
        <button 
          onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
          className="p-2 text-on-surface/40 hover:text-on-surface hover:bg-surface-bright rounded-xl transition-all relative overflow-hidden group"
        >
          <AnimatePresence mode="wait">
            {theme === 'dark' ? (
              <motion.div
                key="moon"
                initial={{ y: 20, opacity: 0 }}
                animate={{ y: 0, opacity: 1 }}
                exit={{ y: -20, opacity: 0 }}
              >
                <Moon className="w-4 h-4" />
              </motion.div>
            ) : (
              <motion.div
                key="sun"
                initial={{ y: 20, opacity: 0 }}
                animate={{ y: 0, opacity: 1 }}
                exit={{ y: -20, opacity: 0 }}
              >
                <Sun className="w-4 h-4" />
              </motion.div>
            )}
          </AnimatePresence>
        </button>

        <div className="w-px h-6 bg-outline mx-1 opacity-50"></div>

        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 px-3 py-1.5 bg-primary/5 border border-primary/20 rounded-full group cursor-pointer hover:bg-primary/10 transition-all">
            <div className="w-1.5 h-1.5 bg-primary rounded-full animate-pulse shadow-[0_0_8px_rgba(var(--color-primary-rgb),0.5)]"></div>
            <span className="text-[9px] text-primary font-bold uppercase tracking-widest">Engine v2.0.4</span>
          </div>
        </div>
      </div>
    </header>
  );
}
