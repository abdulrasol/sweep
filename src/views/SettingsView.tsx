import { Moon, Sun, Palette, Monitor, Bell, Shield, Terminal } from 'lucide-react';
import { useState } from 'react';

interface SettingsViewProps {
  accent: string;
  onAccentChange: (accent: string) => void;
  theme: 'light' | 'dark';
  onThemeChange: (theme: 'light' | 'dark') => void;
}

export default function SettingsView({ accent, onAccentChange, theme, onThemeChange }: SettingsViewProps) {
  const colors = [
    { id: 'emerald', label: 'Emerald', hex: '#10b981', class: 'bg-[#10b981]' },
    { id: 'blue', label: 'Sapphire', hex: '#3b82f6', class: 'bg-[#3b82f6]' },
    { id: 'purple', label: 'Amethyst', hex: '#a855f7', class: 'bg-[#a855f7]' },
    { id: 'rose', label: 'Crimson', hex: '#f43f5e', class: 'bg-[#f43f5e]' },
    { id: 'amber', label: 'Amber', hex: '#f59e0b', class: 'bg-[#f59e0b]' },
  ];

  return (
    <div className="max-w-4xl mx-auto py-12 px-6 space-y-8">
      <section className="space-y-2">
        <h2 className="text-2xl font-bold tracking-tight text-on-surface">System Preferences</h2>
        <p className="text-sm text-on-surface/40 max-w-xl">
          Customize the environment orchestration and visual parameters of the cleaning engine.
        </p>
      </section>

      <div className="grid grid-cols-1 gap-6">
        {/* Appearance Section */}
        <div className="bg-surface-container border border-outline rounded-2xl overflow-hidden shadow-sm">
          <div className="px-6 py-4 border-b border-outline bg-surface-bright/30 flex items-center gap-3">
            <Palette className="w-4 h-4 text-primary" />
            <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface/60">Appearance & Identity</h3>
          </div>
          
          <div className="p-6 space-y-8">
            {/* Theme Mode */}
            <div className="flex items-center justify-between">
              <div className="space-y-1">
                <p className="text-sm font-semibold">Interface Mode</p>
                <p className="text-xs text-on-surface/40">Switch between dark for focus or light for clarity.</p>
              </div>
              <div className="flex bg-surface-dim border border-outline rounded-xl p-1">
                <button 
                  onClick={() => onThemeChange('light')}
                  className={`flex items-center gap-2 px-4 py-2 rounded-lg text-xs font-bold transition-all ${theme === 'light' ? 'bg-surface-bright text-on-surface shadow-sm' : 'text-on-surface/30 hover:text-on-surface/60'}`}
                >
                  <Sun className="w-3.5 h-3.5" /> Light
                </button>
                <button 
                  onClick={() => onThemeChange('dark')}
                  className={`flex items-center gap-2 px-4 py-2 rounded-lg text-xs font-bold transition-all ${theme === 'dark' ? 'bg-surface-bright text-on-surface shadow-sm' : 'text-on-surface/30 hover:text-on-surface/60'}`}
                >
                  <Moon className="w-3.5 h-3.5" /> Dark
                </button>
              </div>
            </div>

            <div className="h-px bg-outline/50" />

            {/* Accent Color */}
            <div className="space-y-4">
              <div className="space-y-1">
                <p className="text-sm font-semibold">Accent Palette</p>
                <p className="text-xs text-on-surface/40">Select the primary system signal color.</p>
              </div>
              <div className="flex flex-wrap gap-3">
                {colors.map((color) => (
                  <button
                    key={color.id}
                    onClick={() => onAccentChange(color.id)}
                    className={`group flex items-center gap-3 px-4 py-3 rounded-xl border transition-all ${accent === color.id ? 'border-primary bg-primary/[0.03]' : 'border-outline hover:border-outline-variant bg-surface-dim'}`}
                  >
                    <div 
                      className={`w-4 h-4 rounded-full ${color.class}`} 
                      style={{ boxShadow: accent === color.id ? `0 0 10px ${color.hex}66` : 'none' }}
                    />
                    <span className={`text-xs font-bold ${accent === color.id ? 'text-on-surface' : 'text-on-surface/40'}`}>{color.label}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Console & Behavior */}
        <div className="bg-surface-container border border-outline rounded-2xl overflow-hidden opacity-50 cursor-not-allowed">
          <div className="px-6 py-4 border-b border-outline bg-surface-bright/30 flex items-center gap-3">
            <Terminal className="w-4 h-4 text-on-surface/40" />
            <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface/60">Console & Behavior</h3>
          </div>
          <div className="p-6 space-y-6">
            <div className="flex items-center justify-between">
              <div>
                 <p className="text-sm font-semibold">Verbose Logging</p>
                 <p className="text-xs text-on-surface/40">Display detailed system-level orchestration logs.</p>
              </div>
              <div className="w-10 h-5 bg-outline rounded-full relative">
                <div className="absolute left-1 top-1 w-3 h-3 bg-white rounded-full" />
              </div>
            </div>
            <div className="flex items-center justify-between">
              <div>
                 <p className="text-sm font-semibold">Automatic Purge</p>
                 <p className="text-xs text-on-surface/40">Clean detected artifacts automatically after scan.</p>
              </div>
              <div className="w-10 h-5 bg-outline rounded-full relative">
                <div className="absolute left-1 top-1 w-3 h-3 bg-white rounded-full" />
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="flex justify-end pt-4">
        <button className="px-10 py-3 bg-on-surface text-surface rounded-xl font-bold text-xs uppercase tracking-[0.2em] hover:brightness-90 transition-all shadow-lg active:scale-95">
          Save Changes
        </button>
      </div>
    </div>
  );
}
