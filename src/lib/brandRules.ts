export type BrandKey = string;

export const BRAND_RULES: { test: RegExp; key: BrandKey }[] = [
  { test: /android studio/, key: 'androidstudio' },
  { test: /gradle/, key: 'gradle' },
  { test: /conda|miniconda|anaconda|mambaforge|miniforge/, key: 'conda' },
  { test: /poetry/, key: 'poetry' },
  { test: /\buv\b/, key: 'uv' },
  { test: /playwright/, key: 'playwright' },
  { test: /cypress/, key: 'cypress' },
  { test: /puppeteer/, key: 'puppeteer' },
  { test: /electron/, key: 'electron' },
  { test: /node \/ pnpm/, key: 'node' },
  { test: /\bpnpm\b/, key: 'pnpm' },
  { test: /\byarn\b/, key: 'yarn' },
  { test: /\bbun\b/, key: 'bun' },
  { test: /\bnpm\b|\bnpx\b/, key: 'npm' },
  { test: /\bnode\b|\bnvm\b|\bfnm\b|\bvolta\b/, key: 'node' },
  { test: /android|avd|emulator|system image|build-tools/, key: 'android' },
  { test: /flutter|\bdart\b|pub cache/, key: 'flutter' },
  { test: /cargo|\brust\b|rustup|toolchain/, key: 'rust' },
  { test: /composer/, key: 'composer' },
  { test: /\bphp\b|vendor & cache/, key: 'php' },
  { test: /laravel/, key: 'laravel' },
  { test: /\bgo\b/, key: 'go' },
  { test: /\bpip3?\b|python|pyenv|virtual environments/, key: 'python' },
  { test: /cocoapods|\bpods?\b/, key: 'cocoapods' },
  { test: /xcode|deriveddata|simulator|device support|swift/, key: 'xcode' },
  { test: /homebrew|\bbrew\b/, key: 'homebrew' },
  { test: /docker|orbstack|colima/, key: 'docker' },
  { test: /hugging/, key: 'huggingface' },
  { test: /torch/, key: 'pytorch' },
  { test: /unity/, key: 'unity' },
  { test: /unreal/, key: 'unreal' },
  { test: /\.net|c#|nuget|dotnet/, key: 'dotnet' },
  { test: /ruby|rails/, key: 'rails' },
  { test: /cmake|c\+\+/, key: 'cpp' },
  { test: /claude|anthropic/, key: 'claude' },
  { test: /chatgpt|openai|codex/, key: 'openai' },
  { test: /gemini|antigravity/, key: 'gemini' },
  { test: /copilot/, key: 'copilot' },
  { test: /cursor/, key: 'cursor' },
  { test: /vs code|vscode|visual studio code/, key: 'vscode' },
  { test: /jetbrains|intellij|webstorm|pycharm/, key: 'jetbrains' },
  { test: /windsurf|cline|\broo\b|kilo|continue|trae|kiro|\bamp\b|opencode|aider|qwen|deepseek|kimi/, key: 'ai' },
  { test: /telegram/, key: 'telegram' },
  { test: /discord/, key: 'discord' },
  { test: /spotify/, key: 'spotify' },
  { test: /vagrant/, key: 'vagrant' },
];

export function brandKey(fileType: string, type: string): BrandKey | null {
  const haystack = `${fileType} ${type}`.toLowerCase();
  for (const rule of BRAND_RULES) {
    if (rule.test.test(haystack)) {
      return rule.key;
    }
  }
  return null;
}
