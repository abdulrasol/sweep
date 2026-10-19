<!-- markdownlint-disable MD033 MD036 -->
# SWEEP

<div align="center">
  <img src="public/sweep.png" alt="Sweep Logo" style="width: 128px; border-radius: 24px;" />
  <h1>SWEEP</h1>
</div>

## A High-Performance Technical Utility for Modern Developers

Built with ❤️ and a team of AI pair programmers: **Google Gemini**, **Claude** and **DeepSeek**.

![Built with Gemini](https://img.shields.io/badge/Built%20with-Gemini-4285F4?style=for-the-badge&logo=googlegemini&logoColor=white) ![Built with Claude](https://img.shields.io/badge/Built%20with-Claude-D97757?style=for-the-badge&logo=claude&logoColor=white) ![Built with DeepSeek](https://img.shields.io/badge/Built%20with-DeepSeek-4D6BFE?style=for-the-badge) ![Powered by Tauri v2](https://img.shields.io/badge/Powered%20by-Tauri%20v2-FF8239?style=for-the-badge&logo=tauri) ![Language - Rust / React](https://img.shields.io/badge/Language-Rust%20%2F%20React-000000?style=for-the-badge&logo=rust)

---

## 📖 The Story Behind Sweep

Yesterday at my company, a classmate asked me to help him run a Flutter application we were working on. I plugged in his device and ran `flutter run ios`. Suddenly, the process crashed with a "No enough space" error.

We both assumed his iPhone was full. He checked, and it had plenty of storage. I checked my Mac, and to my horror, I only had **1 GB of free space left**. My machine was suffocating under years of hidden developer artifacts.

I asked **Google Gemini** for help. It pointed me to deep system paths like `~/Library/Developer/Xcode/DerivedData`. That moment changed everything. I realized that every developer faces this "stealth storage eater" problem.

I decided to create a tool that isn't just a simple script, but a professional, high-end command center to reclaim our workspaces. I called it **Sweep**.

*Sweep started as a pair-programming session with Google Gemini, and grew with **Claude** and **DeepSeek** joining in, especially on the safety work that makes a tool which deletes files trustworthy.*

---

## 🚀 Features

- **Multi-Environment Deep Cleaning**:
  - **Mobile**: Flutter / Dart, Android (Gradle, AVD), Xcode (DerivedData, DeviceSupport, Archives, Simulators).
  - **Web**: Node.js (`node_modules`), pnpm / npm / Yarn caches, Next.js caches, test browsers.
  - **Systems**: Rust (`target`, old toolchains), Python / Conda, Go, PHP, .NET, C++.
  - **Tools**: Homebrew, Docker, editor workspace storage, old app and extension versions.
  - **AI**: Local model caches and AI tool histories.
- **Safety levels on every item**: `SAFE` (regenerates on its own), `REVIEW` (regenerates but costs time or a download), `DANGER` (needs typed confirmation).
- **Never touches your code**: folders inside a project that contain files tracked by git are skipped, and a fixed blocklist protects system and personal folders.
- **Official tools first**: where a tool has its own cleanup command (`xcrun simctl`, `brew cleanup`, `docker prune`, ...), Sweep uses it instead of deleting by hand.
- **Trash or permanent**: choose whether removed items go to the Trash or are deleted permanently.
- **Real sizes**: sizes reflect the space actually freed on disk (hardlinks and clones are not double-counted).
- **Blocks deletion while the related app is running** (e.g. Xcode, Android Studio).
- **Review before purge**: nothing is deleted without your confirmation.

---

## 📥 Install (macOS)

Download the latest `.dmg` from the [Releases](https://github.com/abdulrasol/sweep/releases) page, open it and drag **Sweep** into **Applications**.

Releases are **signed with an Apple Developer ID and notarized by Apple**, so Sweep opens normally with no Gatekeeper warning and no terminal commands.

### Full Disk Access (recommended)

Some caches live in folders that macOS protects. To let Sweep scan them, open **System Settings → Privacy & Security → Full Disk Access** and enable **Sweep**. Sweep shows a shortcut to this pane when access is missing; without it, those folders are simply skipped.

---

## 🛠 Tech Stack

- **Backend**: Rust (Tauri v2) for high-performance file system operations.
- **Frontend**: React + TypeScript.
- **Styling**: Tailwind CSS with custom Glassmorphism utilities.
- **Animations**: Motion (Framer Motion).
- **Icons**: Lucide React & React Icons (Si).

---

## 🧑‍💻 Building from Source

### Prerequisites

- [Node.js](https://nodejs.org/) (v18+) and [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/) (v1.75+)
- **System Dependencies**:
  - **Mac**: Xcode Command Line Tools.
  - **Linux**: `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libssl-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`.
  - **Windows**: WebView2 and C++ Build Tools.

### 1. Clone the Repository

```bash
git clone https://github.com/abdulrasol/sweep.git
cd sweep
```

### 2. Install Dependencies

```bash
pnpm install
```

### 3. Run in Development

```bash
cargo tauri dev
```

### 4. Build for Production

```bash
cargo tauri build
```

Artifacts:

- **macOS**: `target/release/bundle/dmg/`
- **Windows**: `target/release/bundle/msi/`
- **Linux**: `target/release/bundle/deb/`

> Local builds are not notarized. If you build Sweep yourself, macOS may ask you to confirm the first launch (right-click → **Open**).

---

## 📜 License & Credits

Developed by **Abdulrasool Al-Hilo**, pair-programmed with **Google Gemini**, **Claude** (Anthropic) and **DeepSeek**.

This project is a testament to the power of Agentic AI, built by a developer who just wanted their 1GB of space back.

**Disclaimer**: Sweep can delete files permanently. Always review your targets before cleaning.

---
<div align="center">
  <p><i>Sweep: Reclaiming your disk, one artifact at a time.</i></p>
</div>
