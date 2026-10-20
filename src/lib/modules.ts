import type { ComponentType } from 'react';
import { HardDrive, Monitor, Bot } from 'lucide-react';
import {
  SiFlutter, SiNodedotjs, SiRust, SiPython, SiDocker, SiHomebrew, SiPhp, SiKotlin, SiGo,
  SiCplusplus, SiUnity, SiDotnet, SiRubyonrails, SiUnrealengine, SiHuggingface, SiVagrant,
  SiDiscord, SiXcode, SiCocoapods, SiOpenjdk, SiTerraform, SiSlack,
} from 'react-icons/si';

export interface ModuleDef {
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

// review: true when the module can produce Review or Danger items (they are never selected automatically). Modules with only Safe items have no badge.
export const PROJECT_MODULES: ModuleDef[] = [
  { id: 'flutter', title: 'Flutter / Dart', icon: SiFlutter, color: '#02569B', description: 'build, .dart_tool and Pods in Flutter projects, plus the pub cache through dart.', review: true },
  { id: 'node', title: 'Node / pnpm', icon: SiNodedotjs, color: '#339933', description: 'node_modules and build output, npm, pnpm, Yarn, Bun and Deno caches, test browsers and old Node versions.', review: true },
  { id: 'rust', title: 'Rust / Cargo', icon: SiRust, color: '#CE422B', description: 'target folders, Cargo registry caches and toolchains other than your default.', review: true },
  { id: 'rust_incremental', title: 'Rust incremental only', icon: SiRust, color: '#CE422B', description: 'For projects you still work on: only target/*/incremental, so compiled dependencies stay.' },
  { id: 'android', title: 'Android / Kotlin', icon: SiKotlin, color: '#7F52FF', description: 'Gradle builds and caches, unused SDK system images, old build-tools and old Android Studio versions.', review: true },
  { id: 'php', title: 'PHP / Laravel', icon: SiPhp, color: '#777BB4', description: 'vendor folders, Laravel storage logs, and the global Composer cache.', review: true },
  { id: 'python_venv', title: 'Python environments', icon: SiPython, color: '#3776AB', description: 'Project .venv and venv folders with a pyvenv.cfg. Packages must be installed again.', review: true },
  { id: 'dotnet', title: '.NET / C#', icon: SiDotnet, color: '#512BD4', description: 'bin and obj folders, and the NuGet package cache.' },
  { id: 'ruby', title: 'Ruby on Rails', icon: SiRubyonrails, color: '#CC0000', description: 'vendor/bundle and tmp caches.', review: true },
  { id: 'cpp', title: 'C++ / CMake', icon: SiCplusplus, color: '#00599C', description: 'CMake build and out folders.' },
  { id: 'unity', title: 'Unity', icon: SiUnity, color: 'currentColor', description: 'Library and Temp folders in Unity projects.', review: true },
  { id: 'unreal', title: 'Unreal Engine', icon: SiUnrealengine, color: 'currentColor', description: 'Intermediate, Saved and Binaries folders.', review: true },
  { id: 'go', title: 'Go', icon: SiGo, color: '#00ADD8', description: 'The Go build cache, and the module cache through go clean -modcache.', review: true },
];

export const GLOBAL_MODULES: ModuleDef[] = [
  { id: 'xcode', title: 'Xcode', icon: SiXcode, color: '#147EFB', description: 'DerivedData, device support, archives, previews, simulators and Swift packages.', review: true },
  { id: 'cocoapods', title: 'CocoaPods', icon: SiCocoapods, color: '#EE3322', description: 'Pod download cache and spec repos.', review: true },
  { id: 'java', title: 'Java / Maven', icon: SiOpenjdk, color: '#ED8B00', description: 'Maven local repository.', review: true },
  { id: 'devops', title: 'Terraform', icon: SiTerraform, color: '#844FBA', description: 'Terraform plugin cache.' },
  { id: 'homebrew', title: 'Homebrew', icon: SiHomebrew, color: '#FBB040', description: 'Downloaded bottles, and old versions of installed formulae and casks.', review: true },
  { id: 'ai_assistants', title: 'AI assistants', icon: Bot, color: '#D97757', description: 'Caches from Claude, ChatGPT, Codex, Cursor, Gemini, Copilot and more. Chat history is marked Danger.', review: true },
  { id: 'ai', title: 'AI / ML models', icon: SiHuggingface, color: '#FFB000', description: 'Hugging Face, Ollama, LM Studio and Jan models, one line per model, plus PyTorch caches. Login tokens and keys are never touched.', review: true },
  { id: 'editors', title: 'Editor extensions', icon: Monitor, color: '#0EA5E9', description: 'Editor caches, leftover project data and old versions for VS Code, Cursor, Windsurf and JetBrains.', review: true },
  { id: 'python', title: 'Python / Conda', icon: SiPython, color: '#3776AB', description: 'Conda environments (marked Danger), the Conda package cache, pip/uv/Poetry caches, and Python cache folders in projects.', review: true },
  { id: 'os', title: 'Vagrant', icon: SiVagrant, color: '#1563FF', description: 'Vagrant boxes, marked Danger.', review: true },
  { id: 'social', title: 'Chat and media apps', icon: SiDiscord, color: '#5865F2', description: 'Telegram media, Discord and Spotify caches.', review: true },
  { id: 'work_apps', title: 'Work apps', icon: SiSlack, color: '#4A154B', description: 'Caches from Slack, Zoom, Teams, Notion, Postman and Figma. Your sign-ins and messages are never touched.' },
  { id: 'adobe', title: 'Adobe', icon: HardDrive, color: '#FF3D00', description: 'After Effects and Premiere media cache.', review: true },
  { id: 'os_system', title: 'App caches', icon: HardDrive, color: 'currentColor', description: 'Each folder in ~/Library/Caches over 10 MB, listed for review.', review: true },
  { id: 'system', title: 'System', icon: HardDrive, color: 'currentColor', description: 'System logs, iPhone backups, the Trash and old installers in Downloads. Backups are marked Danger. Downloads files are always Review.', review: true },
  { id: 'docker', title: 'Docker', icon: SiDocker, color: '#2496ED', description: 'Build cache, stopped containers and unused images through the docker CLI. Volumes are never touched.', review: true },
];

const TITLES = new Map(
  [...PROJECT_MODULES, ...GLOBAL_MODULES].map(m => [m.id, m.title]),
);

/** Card title for a module id, or the id itself when it is unknown. */
export function moduleTitle(id: string): string {
  return TITLES.get(id) ?? id;
}
