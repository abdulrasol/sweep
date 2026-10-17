import type { ComponentType, CSSProperties } from 'react';
import {
  SiAndroidstudio,
  SiGradle,
  SiAnaconda,
  SiPoetry,
  SiUv,
  SiCypress,
  SiPuppeteer,
  SiElectron,
  SiNodedotjs,
  SiPnpm,
  SiYarn,
  SiBun,
  SiNpm,
  SiAndroid,
  SiFlutter,
  SiRust,
  SiComposer,
  SiLaravel,
  SiPhp,
  SiGo,
  SiPython,
  SiCocoapods,
  SiXcode,
  SiHomebrew,
  SiDocker,
  SiHuggingface,
  SiPytorch,
  SiUnity,
  SiUnrealengine,
  SiDotnet,
  SiRubyonrails,
  SiCplusplus,
  SiClaude,
  SiOpenai,
  SiGooglegemini,
  SiGithubcopilot,
  SiJetbrains,
  SiTelegram,
  SiDiscord,
  SiSpotify,
  SiVagrant,
} from 'react-icons/si';
import {
  Bot,
  Code2,
  Package,
  FolderCode,
  Wrench,
  Puzzle,
  HardDrive,
} from 'lucide-react';
import { brandKey } from './brandRules';

export interface BrandInfo {
  Icon: ComponentType<{ className?: string; style?: CSSProperties }>;
  color: string;
  name: string;
}

export const BRANDS: Record<string, BrandInfo> = {
  androidstudio: { Icon: SiAndroidstudio, color: '#3DDC84', name: 'Android Studio' },
  gradle: { Icon: SiGradle, color: '#1BA8CB', name: 'Gradle' },
  conda: { Icon: SiAnaconda, color: '#44A833', name: 'Conda' },
  poetry: { Icon: SiPoetry, color: '#60A5FA', name: 'Poetry' },
  uv: { Icon: SiUv, color: '#DE5FE9', name: 'uv' },
  playwright: { Icon: Package, color: '#2EAD33', name: 'Playwright' },
  cypress: { Icon: SiCypress, color: '#69D3A7', name: 'Cypress' },
  puppeteer: { Icon: SiPuppeteer, color: '#40B5A4', name: 'Puppeteer' },
  electron: { Icon: SiElectron, color: '#47848F', name: 'Electron' },
  node: { Icon: SiNodedotjs, color: '#339933', name: 'Node.js' },
  pnpm: { Icon: SiPnpm, color: '#F69220', name: 'pnpm' },
  yarn: { Icon: SiYarn, color: '#2C8EBB', name: 'Yarn' },
  bun: { Icon: SiBun, color: 'currentColor', name: 'Bun' },
  npm: { Icon: SiNpm, color: '#CB3837', name: 'npm' },
  android: { Icon: SiAndroid, color: '#3DDC84', name: 'Android' },
  flutter: { Icon: SiFlutter, color: '#02569B', name: 'Flutter' },
  rust: { Icon: SiRust, color: '#CE422B', name: 'Rust' },
  composer: { Icon: SiComposer, color: '#885630', name: 'Composer' },
  laravel: { Icon: SiLaravel, color: '#FF2D20', name: 'Laravel' },
  php: { Icon: SiPhp, color: '#777BB4', name: 'PHP' },
  go: { Icon: SiGo, color: '#00ADD8', name: 'Go' },
  python: { Icon: SiPython, color: '#3776AB', name: 'Python' },
  cocoapods: { Icon: SiCocoapods, color: '#EE3322', name: 'CocoaPods' },
  xcode: { Icon: SiXcode, color: '#147EFB', name: 'Xcode' },
  homebrew: { Icon: SiHomebrew, color: '#FBB040', name: 'Homebrew' },
  docker: { Icon: SiDocker, color: '#2496ED', name: 'Docker' },
  huggingface: { Icon: SiHuggingface, color: '#FFB000', name: 'Hugging Face' },
  pytorch: { Icon: SiPytorch, color: '#EE4C2C', name: 'PyTorch' },
  unity: { Icon: SiUnity, color: 'currentColor', name: 'Unity' },
  unreal: { Icon: SiUnrealengine, color: 'currentColor', name: 'Unreal Engine' },
  dotnet: { Icon: SiDotnet, color: '#512BD4', name: '.NET' },
  rails: { Icon: SiRubyonrails, color: '#CC0000', name: 'Ruby on Rails' },
  cpp: { Icon: SiCplusplus, color: '#00599C', name: 'C++' },
  claude: { Icon: SiClaude, color: '#D97757', name: 'Claude' },
  openai: { Icon: SiOpenai, color: '#10A37F', name: 'OpenAI' },
  gemini: { Icon: SiGooglegemini, color: '#8E75B2', name: 'Gemini' },
  copilot: { Icon: SiGithubcopilot, color: 'currentColor', name: 'GitHub Copilot' },
  cursor: { Icon: Bot, color: 'currentColor', name: 'Cursor' },
  vscode: { Icon: Code2, color: '#007ACC', name: 'VS Code' },
  jetbrains: { Icon: SiJetbrains, color: 'currentColor', name: 'JetBrains' },
  ai: { Icon: Bot, color: '#D97757', name: 'AI' },
  telegram: { Icon: SiTelegram, color: '#26A5E4', name: 'Telegram' },
  discord: { Icon: SiDiscord, color: '#5865F2', name: 'Discord' },
  spotify: { Icon: SiSpotify, color: '#1DB954', name: 'Spotify' },
  vagrant: { Icon: SiVagrant, color: '#1563FF', name: 'Vagrant' },
};

const GROUP_FALLBACK: Record<string, ComponentType<{ className?: string; style?: CSSProperties }>> = {
  'Projects': FolderCode,
  'Dev Tools': Wrench,
  'AI Tools': Bot,
  'Editors': Puzzle,
  'System & Apps': HardDrive,
};

export function brandFor(item: Pick<CleanupItem, 'file_type' | 'type' | 'group'>): BrandInfo {
  const key = brandKey(item.file_type, item.type);
  if (key && BRANDS[key]) {
    return BRANDS[key];
  }

  const FallbackIcon = GROUP_FALLBACK[item.group] ?? Package;
  return {
    Icon: FallbackIcon,
    color: 'currentColor',
    name: item.group,
  };
}

export function BrandTile({ item }: { item: Pick<CleanupItem, 'file_type' | 'type' | 'group'> }) {
  const brand = brandFor(item);
  const Icon = brand.Icon;
  const isCurrent = brand.color === 'currentColor';

  return (
    <div
      aria-hidden
      title={brand.name}
      className={`w-9 h-9 rounded-lg flex items-center justify-center shrink-0 ${
        isCurrent ? 'bg-surface-bright text-on-surface/70' : ''
      }`}
      style={isCurrent ? undefined : { backgroundColor: `${brand.color}1f`, color: brand.color }}
    >
      <Icon className="w-[18px] h-[18px]" />
    </div>
  );
}
