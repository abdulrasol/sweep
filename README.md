<!-- markdownlint-disable MD033 MD036 -->
# SWEEP

<div align="center">
  <img src="public/sweep.png" alt="Sweep Logo" style="width: 128px; border-radius: 24px;" />
  <h1>SWEEP</h1>
</div>

## A High-Performance Technical Utility for Modern Developers

Built with ❤️ using **Google Gemini (Antigravity)**

![Built with Gemini 3.5 Flash](https://img.shields.io/badge/Built%20with-Gemini%203.5%20Flash-blue?style=for-the-badge&logo=google-gemini) ![Powered by Tauri v2](https://img.shields.io/badge/Powered%20by-Tauri%20v2-FF8239?style=for-the-badge&logo=tauri) ![Language - Rust / React](https://img.shields.io/badge/Language-Rust%20%2F%20React-000000?style=for-the-badge&logo=rust)

---

## 📖 The Story Behind Sweep

Yesterday at my company, a classmate asked me to help him run a Flutter application we were working on. I plugged in his device and ran `flutter run ios`. Suddenly, the process crashed with a "No enough space" error.

We both assumed his iPhone was full. He checked, and it had plenty of storage. I checked my Mac, and to my horror, I only had **1 GB of free space left**. My machine was suffocating under years of hidden developer artifacts.

I asked **Google Gemini** (running on the cloud free model) for help. It pointed me to deep system paths like `~/Library/Developer/Xcode/DerivedData`. That moment changed everything. I realized that every developer faces this "stealth storage eater" problem.

I decided to create a tool that isn't just a simple script, but a professional, high-end command center to reclaim our workspaces. I called it **Sweep**.

*Note: This entire project was pair-programmed with Google Gemini. When the cloud limits hit, we switched to Gemini 1.5 Flash to keep the momentum going!*

---

## 🚀 Features

- **Multi-Environment Deep Cleaning**:
  - **Mobile**: Flutter, Android (AVD/Gradle), Xcode (DerivedData).
  - **Web**: Node.js (node_modules), PNPM stores, Next.js caches.
  - **Systems**: Rust (Cargo target), Python (Conda/Pip), PHP, .NET, C++, Go.
  - **Specialized**: Unity, Unreal Engine, Adobe Media Caches, Docker.
- **Global Runtime Status**: A real-time sidebar dashboard showing:
  - OS Identity & Version.
  - CPU Utilization.
  - RAM Pressure.
  - Available Disk Space.
- **Smart Target Scope**: Select your root development directory and let Sweep find the garbage.
- **Glassmorphism UI**: A premium, dark-mode interface built for modern aesthetics.
- **Safety First**: Review every single file before the final purge.

---

## 🛠 Tech Stack

- **Backend**: Rust (Tauri v2) for high-performance file system operations.
- **Frontend**: React + TypeScript.
- **Styling**: Tailwind CSS with custom Glassmorphism utilities.
- **Animations**: Motion (Framer Motion).
- **Icons**: Lucide React & React Icons (Si).

---

## 📦 Installation & Setup

### Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
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

---

## 🏗 Building for Production

Sweep is designed to be cross-platform. To build a standalone executable for your current OS:

```bash
cargo tauri build
```

### Artifact Locations
- **macOS**: `src-tauri/target/release/bundle/dmg/`
- **Windows**: `src-tauri/target/release/bundle/msi/`
- **Linux**: `src-tauri/target/release/bundle/deb/`

---

## 📜 License & Credits

Built with **Vide Coding** techniques and **Google Gemini**.

This project is a testament to the power of Agentic AI. Developed by **Antigravity** in collaboration with a developer who just wanted their 1GB of space back.

**Disclaimer**: Sweep performs permanent deletions. Always review your targets before initiating a Global Scan.

---
<div align="center">
  <p><i>Sweep: Reclaiming your disk, one artifact at a time.</i></p>
</div>
