import { useState, useEffect, type ComponentType } from 'react';
import { FolderOpen, HardDrive, Monitor, Bot, ScanSearch } from 'lucide-react';
import {
  SiFlutter, SiNodedotjs, SiRust, SiPython, SiDocker, SiHomebrew, SiPhp, SiKotlin, SiGo,
  SiCplusplus, SiUnity, SiDotnet, SiRubyonrails, SiUnrealengine, SiHuggingface, SiVagrant,
  SiDiscord, SiXcode, SiCocoapods,
} from 'react-icons/si';
import { invoke } from '../lib/tauri';

interface ScanViewProps {
  initialPath: string;
  initialModules: string[];
  onScan: (path: string, modules: string[]) => void;
}

interface ModuleDef {
  id: string;
  title: string;
  icon: ComponentType<{ className?: string }>;
  description: string;
  /** Brand color for the icon. */
  color: string;
  /** Can list items that need a closer look before removing. */
  review?: boolean;
  /** Listed for completeness; the engine has no rules for it yet. */
  soon?: boolean;
}

const PROJECT_MODULES: ModuleDef[] = [
  { id: 'flutter', title: 'Flutter / Dart', icon: SiFlutter, color: '#02569B', description: 'build, .dart_tool and iOS/macOS Pods inside Flutter projects.' },
  { id: 'node', title: 'Node / pnpm', icon: SiNodedotjs, color: '#339933', description: 'node_modules, .next and other build output, plus the pnpm store.' },
  { id: 'rust', title: 'Rust / Cargo', icon: SiRust, color: '#CE422B', description: 'target folders in Cargo projects.' },
  { id: 'android', title: 'Android / Kotlin', icon: SiKotlin, color: '#7F52FF', description: 'Gradle build folders and emulator images.', review: true },
  { id: 'php', title: 'PHP / Laravel', icon: SiPhp, color: '#777BB4', description: 'vendor folders and Laravel storage logs.' },
  { id: 'dotnet', title: '.NET / C#', icon: SiDotnet, color: '#512BD4', description: 'bin and obj folders, and the NuGet package cache.' },
  { id: 'ruby', title: 'Ruby on Rails', icon: SiRubyonrails, color: '#CC0000', description: 'vendor/bundle and tmp caches.' },
  { id: 'cpp', title: 'C++ / CMake', icon: SiCplusplus, color: '#00599C', description: 'CMake build and out folders.' },
  { id: 'unity', title: 'Unity', icon: SiUnity, color: 'currentColor', description: 'Library and Temp folders in Unity projects.' },
  { id: 'unreal', title: 'Unreal Engine', icon: SiUnrealengine, color: 'currentColor', description: 'Intermediate, Saved and Binaries folders.' },
  { id: 'go', title: 'Go', icon: SiGo, color: '#00ADD8', description: 'Module and build caches.', soon: true },
];

const GLOBAL_MODULES: ModuleDef[] = [
  { id: 'xcode', title: 'Xcode', icon: SiXcode, color: '#147EFB', description: 'DerivedData, device support, archives, previews and simulators.', review: true },
  { id: 'cocoapods', title: 'CocoaPods', icon: SiCocoapods, color: '#EE3322', description: 'Pod download cache and spec repos.', review: true },
  { id: 'homebrew', title: 'Homebrew', icon: SiHomebrew, color: '#FBB040', description: 'Downloaded bottles and formulae.' },
  { id: 'ai_assistants', title: 'AI assistants', icon: Bot, color: '#D97757', description: 'Caches from Claude, ChatGPT, Codex, Cursor, Gemini, Copilot and more. Chat history is marked Danger.', review: true },
  { id: 'ai', title: 'AI / ML models', icon: SiHuggingface, color: '#FFB000', description: 'Hugging Face and PyTorch model caches.', review: true },
  { id: 'editors', title: 'Editor extensions', icon: Monitor, color: '#0EA5E9', description: 'Old extension versions in VS Code, Cursor, Windsurf and others.' },
  { id: 'python', title: 'Python / Conda', icon: SiPython, color: '#3776AB', description: 'Conda environments, marked Danger.', review: true },
  { id: 'os', title: 'Vagrant', icon: SiVagrant, color: '#1563FF', description: 'Vagrant boxes, marked Danger.', review: true },
  { id: 'social', title: 'Chat and media apps', icon: SiDiscord, color: '#5865F2', description: 'Telegram media, Discord and Spotify caches.', review: true },
  { id: 'adobe', title: 'Adobe', icon: HardDrive, color: '#FF3D00', description: 'After Effects and Premiere media cache.', review: true },
  { id: 'os_system', title: 'App caches', icon: HardDrive, color: 'currentColor', description: 'Each folder in ~/Library/Caches over 10 MB, listed for review.', review: true },
  { id: 'docker', title: 'Docker', icon: SiDocker, color: '#2496ED', description: 'Cleanup through the docker CLI.', soon: true },
];

const ALL_MODULES = [...PROJECT_MODULES, ...GLOBAL_MODULES];
const AVAILABLE_IDS = ALL_MODULES.filter(m => !m.soon).map(m => m.id);

function ModuleCard({ mod, on, onToggle }: { mod: ModuleDef; on: boolean; onToggle: () => void }) {
  const Icon = mod.icon;
  return (
    <button
      role="switch"
      aria-checked={on}
      disabled={mod.soon}
      onClick={onToggle}
      className={`text-left flex gap-3 p-3.5 rounded-xl border transition-colors
        ${mod.soon ? 'border-dashed border-outline opacity-60 cursor-not-allowed'
          : on ? 'border-primary/50 bg-primary/[0.05]' : 'border-outline bg-surface-container hover:border-outline-variant'}`}
    >
      <span className={`mt-0.5 shrink-0 transition-opacity ${on || mod.soon ? '' : 'opacity-40'}`} style={{ color: mod.color }}>
        <Icon className="w-5 h-5" />
      </span>
      <span className="flex-1 min-w-0">
        <span className="flex items-center gap-2">
          <span className="text-sm font-medium truncate">{mod.title}</span>
          {mod.review && !mod.soon && (
            <span className="text-[10px] px-1.5 py-px rounded border border-warning/40 text-warning">Review</span>
          )}
          {mod.soon && <span className="text-[10px] px-1.5 py-px rounded bg-surface-bright text-on-surface/60">Coming later</span>}
        </span>
        <span className="block mt-0.5 text-xs text-on-surface/55 leading-snug">{mod.description}</span>
      </span>
      {!mod.soon && (
        <span className={`mt-0.5 w-8 h-[18px] shrink-0 rounded-full p-0.5 transition-colors ${on ? 'bg-primary' : 'bg-outline-variant'}`} aria-hidden>
          <span className={`block w-3.5 h-3.5 rounded-full bg-white shadow-sm transition-transform ${on ? 'translate-x-3.5' : ''}`} />
        </span>
      )}
    </button>
  );
}

export default function ScanView({ initialPath, initialModules, onScan }: ScanViewProps) {
  const [path, setPath] = useState(initialPath);
  const [enabled, setEnabled] = useState<Set<string>>(new Set(initialModules.filter(id => AVAILABLE_IDS.includes(id))));
  const [pickError, setPickError] = useState<string | null>(null);

  useEffect(() => {
    if (initialPath) setPath(initialPath);
  }, [initialPath]);

  const chooseFolder = async () => {
    try {
      const selected = await invoke<string | null>('select_directory');
      if (selected) setPath(selected);
      setPickError(null);
    } catch (err) {
      setPickError(String(err));
    }
  };

  const toggle = (id: string) =>
    setEnabled(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const allOn = enabled.size === AVAILABLE_IDS.length;
  const canScan = enabled.size > 0 && path.trim() !== '';

  const section = (title: string, hint: string, list: ModuleDef[]) => (
    <section className="space-y-3">
      <div>
        <h3 className="text-sm font-semibold">{title}</h3>
        <p className="text-xs text-on-surface/55">{hint}</p>
      </div>
      <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-2.5">
        {list.map(m => <ModuleCard key={m.id} mod={m} on={enabled.has(m.id)} onToggle={() => toggle(m.id)} />)}
      </div>
    </section>
  );

  return (
    <div className="h-full flex flex-col">
      <div className="flex-1 overflow-y-auto">
        <div className="max-w-5xl mx-auto px-6 py-6 space-y-8">
          <header className="space-y-1.5">
            <h2 className="text-xl font-semibold tracking-tight">Choose what to scan</h2>
            <p className="text-sm text-on-surface/60 max-w-2xl">
              Sweep looks for build output inside your projects folder and for caches left by the tools you turn on.
              Nothing is removed until you review it.
            </p>
          </header>

          <section className="space-y-2">
            <label htmlFor="scan-path" className="text-sm font-semibold">Projects folder</label>
            <div className="flex gap-2">
              <input
                id="scan-path"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="/Users/you/Projects"
                spellCheck={false}
                className="flex-1 min-w-0 bg-surface-container border border-outline rounded-lg px-3 py-2 font-mono text-xs outline-none focus:border-primary/60 placeholder:text-on-surface/30"
              />
              <button
                onClick={chooseFolder}
                className="flex items-center gap-2 px-3.5 py-2 rounded-lg border border-outline bg-surface-container text-sm font-medium hover:bg-surface-bright transition-colors"
              >
                <FolderOpen className="w-4 h-4" />
                Choose…
              </button>
            </div>
            {pickError && <p className="text-xs text-error">Could not open the folder picker: {pickError}</p>}
          </section>

          {section('In your projects', 'Build folders found under the projects folder. Folders with files tracked by git are never removed.', PROJECT_MODULES)}
          {section('On this Mac', 'Caches outside your projects, in your home folder and ~/Library.', GLOBAL_MODULES)}
        </div>
      </div>

      <footer className="border-t border-outline bg-surface-container px-6 py-3.5 shrink-0">
        <div className="max-w-5xl mx-auto flex items-center gap-4">
          <span className="text-sm text-on-surface/60 tabular-nums">
            {enabled.size} of {AVAILABLE_IDS.length} on
          </span>
          <button
            onClick={() => setEnabled(allOn ? new Set() : new Set(AVAILABLE_IDS))}
            className="text-sm text-primary hover:underline"
          >
            {allOn ? 'Turn all off' : 'Turn all on'}
          </button>
          <button
            onClick={() => onScan(path.trim(), Array.from(enabled))}
            disabled={!canScan}
            className="ml-auto flex items-center gap-2 px-5 py-2 rounded-lg bg-on-surface text-surface text-sm font-medium hover:opacity-90 transition-opacity disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <ScanSearch className="w-4 h-4" />
            Scan
          </button>
        </div>
      </footer>
    </div>
  );
}
