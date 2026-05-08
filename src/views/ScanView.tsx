import React, { useState, useEffect } from 'react';
import { Folder, Edit, ChevronRight, Apple, HardDrive, Monitor, Coffee, Zap } from 'lucide-react';
import { 
  SiFlutter, 
  SiNodedotjs, 
  SiRust, 
  SiPython, 
  SiDocker, 
  SiHomebrew, 
  SiApple,
  SiPhp,
  SiAndroid,
  SiKotlin,
  SiGo,
  SiCplusplus,
  SiUnity,
  SiDotnet,
  SiRubyonrails,
  SiUnrealengine,
  SiHuggingface,
  SiAnaconda,
  SiVagrant,
  SiDiscord
} from 'react-icons/si';
import { motion } from 'motion/react';
import { invoke } from '../lib/tauriSimulation';

interface ScanViewProps {
  initialPath: string;
  initialModules: string[];
  onInitiate: (path: string, selectedModules: string[]) => void;
}

export default function ScanView({ initialPath, initialModules, onInitiate }: ScanViewProps) {
  const [path, setPath] = useState(initialPath || '/Users/rasol/DevsTools');
  
  const modulesDef = [
    { id: 'flutter', title: 'Flutter / Dart', icon: SiFlutter, description: 'Pub caches, build artifacts, and pods.', path: 'pubspec.yaml', color: 'text-[#02569B]' },
    { id: 'node', title: 'Node / PNPM', icon: SiNodedotjs, description: 'node_modules, .next, and PNPM global stores.', path: 'package.json', color: 'text-[#339933]' },
    { id: 'rust', title: 'Rust / Cargo', icon: SiRust, description: 'Removes heavy target directories and builds.', path: 'Cargo.toml', color: 'text-orange-500' },
    { id: 'ai', title: 'AI / ML Models', icon: SiHuggingface, description: 'Gigabyte-heavy Hugging Face & Torch models.', path: 'Model Cache', color: 'text-[#FFD21E]' },
    { id: 'android', title: 'Android / Kotlin', icon: SiKotlin, description: 'Gradle, Maven .m2, and heavy AVD Emulator images.', path: 'AVD / Gradle', color: 'text-[#7F52FF]' },
    { id: 'python', title: 'Python / Conda', icon: SiPython, description: 'Conda envs, virtualenvs, and pycaches.', path: 'Conda / PIP', color: 'text-[#3776AB]' },
    { id: 'php', title: 'PHP / Laravel', icon: SiPhp, description: 'Vendor, composer caches, and storage logs.', path: 'composer.json', color: 'text-[#777BB4]' },
    { id: 'dotnet', title: '.NET / C#', icon: SiDotnet, description: 'Clears bin/obj folders and NuGet package caches.', path: '*.sln / CS', color: 'text-[#512BD4]' },
    { id: 'unreal', title: 'Unreal Engine', icon: SiUnrealengine, description: 'Massive Intermediate, Saved, and Binaries.', path: '*.uproject', color: 'text-on-surface' },
    { id: 'ruby', title: 'Ruby on Rails', icon: SiRubyonrails, description: 'Targets vendor/bundle and internal tmp caches.', path: 'Gemfile', color: 'text-[#CC0000]' },
    { id: 'unity', title: 'Unity Engine', icon: SiUnity, description: 'Cleans massive Library and Temp project folders.', path: 'ProjectSettings', color: 'text-[#222c37]' },
    { id: 'cpp', title: 'C++ / CMake', icon: SiCplusplus, description: 'Clears build, out, and object directories.', path: 'CMakeLists.txt', color: 'text-[#00599C]' },
    { id: 'go', title: 'Go / Golang', icon: SiGo, description: 'Removes compiled binaries and module caches.', path: 'go.mod', color: 'text-[#00ADD8]' },
    { id: 'adobe', title: 'Adobe Caches', icon: HardDrive, description: 'Heavy After Effects & Premiere media caches.', path: 'Adobe Common', color: 'text-[#FF0000]' },
    { id: 'social', title: 'Comms & Media', icon: SiDiscord, description: 'Telegram, Discord, Slack, and Spotify caches.', path: 'Social Cache', color: 'text-[#5865F2]' },
    { id: 'os', title: 'Virtualization', icon: SiVagrant, description: 'Vagrant boxes, VirtualBox snapshots, and logs.', path: 'VM / Vagrant', color: 'text-[#1563FF]' },
    { id: 'docker', title: 'Docker System', icon: SiDocker, description: 'Prunes dangling images and unused volumes.', path: 'Docker Desktop', color: 'text-[#2496ED]' },
    { id: 'homebrew', title: 'Homebrew', icon: SiHomebrew, description: 'Clears downloaded bottles and formulae.', path: 'Homebrew Cache', color: 'text-[#FBB040]' },
    { id: 'os_system', title: 'System Caches', icon: HardDrive, description: 'OS logs, temp files, and browser caches.', path: 'System Cache', color: 'text-red-400', badge: 'Review' },
  ];

  const [enabledModules, setEnabledModules] = useState<Set<string>>(new Set(initialModules));

  useEffect(() => {
    if (initialModules.length > 0) {
      setEnabledModules(new Set(initialModules));
    }
    if (initialPath) {
      setPath(initialPath);
    }
  }, [initialModules, initialPath]);

  const handleSelectDirectory = async () => {
    try {
      const selected = await invoke<string>('select_directory');
      if (selected) {
        setPath(selected);
      }
    } catch (err) {
      console.error("Directory selection failed:", err);
    }
  };

  const toggleModule = (id: string) => {
    const newEnabled = new Set(enabledModules);
    if (newEnabled.has(id)) newEnabled.delete(id);
    else newEnabled.add(id);
    setEnabledModules(newEnabled);
  };

  return (
    <div className="h-full flex flex-col overflow-hidden">
      {/* Unified Scrollable Area */}
      <div className="flex-1 overflow-y-auto custom-scrollbar">
        <div className="p-8 space-y-10">
          {/* Header & Scope */}
          <div className="space-y-6">
            <section className="space-y-2">
              <h2 className="text-2xl font-bold tracking-tight text-on-surface">Target Scope Configuration</h2>
              <p className="text-sm text-on-surface/40 leading-relaxed max-w-2xl">
                Sweep now supports technical and stealth storage eaters, including virtual machines, emulators, and communication caches.
              </p>
            </section>

            {/* Root Directory Scope */}
            <div className="bg-surface-container border border-outline rounded-2xl overflow-hidden shadow-sm">
              <div className="p-3 bg-surface-bright/30 border-b border-outline flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Folder className="w-3.5 h-3.5 text-primary opacity-80" />
                  <span className="text-[9px] font-bold uppercase tracking-widest text-on-surface/60">Root Directory Scope</span>
                </div>
              </div>
              <div className="p-4 flex items-center gap-4">
                <div className="flex-1 flex items-center gap-3 bg-surface-dim border border-outline px-4 py-2 rounded-xl font-mono text-xs group focus-within:border-primary/50 transition-all text-on-surface/70">
                  <Monitor className="w-3.5 h-3.5 opacity-30" />
                  <input 
                    value={path}
                    onChange={(e: React.ChangeEvent<HTMLInputElement>) => setPath(e.target.value)}
                    className="bg-transparent outline-none flex-1 placeholder:text-on-surface/10"
                  />
                </div>
                <button 
                  onClick={handleSelectDirectory}
                  className="flex items-center gap-2 px-4 py-2 border border-outline rounded-xl hover:bg-surface-bright transition-all text-[10px] font-bold text-on-surface/60 uppercase tracking-widest"
                >
                  <Edit className="w-3 h-3" />
                  <span>Modify</span>
                </button>
              </div>
            </div>
          </div>

          {/* Modules Grid Section */}
          <div className="space-y-4">
            <div className="flex items-center justify-between px-2">
              <h3 className="text-[10px] font-bold uppercase tracking-widest text-on-surface/30">Analysis Modules ({enabledModules.size} Active)</h3>
              <button 
                onClick={() => setEnabledModules(new Set(modulesDef.map(m => m.id)))}
                className="text-[10px] font-bold uppercase tracking-widest text-primary hover:underline"
              >
                Select All Modules
              </button>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
              {modulesDef.map((mod) => {
                const isEnabled = enabledModules.has(mod.id);
                return (
                  <motion.div
                    key={mod.id}
                    whileHover={{ y: -2 }}
                    onClick={() => toggleModule(mod.id)}
                    className={`bg-surface-container border rounded-2xl p-5 space-y-4 relative group transition-all cursor-pointer
                      ${isEnabled ? 'border-primary/40 bg-primary/[0.03]' : 'border-outline opacity-40 grayscale hover:grayscale-0 hover:opacity-70'}
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
                      <div className={`w-10 h-5 rounded-full p-1 transition-colors ${isEnabled ? 'bg-primary' : 'bg-outline-variant'}`}>
                        <div className={`w-3 h-3 bg-white rounded-full transition-transform ${isEnabled ? 'translate-x-5' : 'translate-x-0'}`} />
                      </div>
                    </div>
                    <p className="text-[11px] text-on-surface/40 leading-relaxed min-h-[40px]">
                      {mod.description}
                    </p>
                    {mod.badge && (
                      <div className="flex justify-end">
                        <span className="text-[9px] bg-yellow-500/10 text-yellow-500 px-2 py-0.5 rounded border border-yellow-500/20 font-bold uppercase tracking-tighter shadow-sm">
                          {mod.badge}
                        </span>
                      </div>
                    )}
                  </motion.div>
                );
              })}
            </div>
          </div>
        </div>
      </div>

      {/* Permanently Docked Footer */}
      <div className="p-6 px-8 border-t border-outline bg-surface-dim flex-shrink-0 shadow-[0_-4px_20px_-5px_rgba(0,0,0,0.15)] z-10">
        <div className="max-w-7xl mx-auto flex justify-end">
          <button
            onClick={() => onInitiate(path, Array.from(enabledModules))}
            disabled={enabledModules.size === 0}
            className="group flex items-center gap-4 bg-on-surface text-surface px-10 py-4 rounded-xl font-bold text-xs uppercase tracking-[0.2em] hover:brightness-90 active:scale-[0.98] transition-all shadow-lg disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Zap className="w-4 h-4 fill-current" />
            <span>Initiate Global Scan</span>
            <ChevronRight className="w-4 h-4 transition-transform group-hover:translate-x-1" />
          </button>
        </div>
      </div>
    </div>
  );
}
