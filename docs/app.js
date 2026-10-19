// Sweep website
(function () {
  "use strict";

  document.documentElement.classList.add("js");
  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---------------------------------------------------------------------------
  // Header: border on scroll, mobile menu
  // ---------------------------------------------------------------------------
  var header = document.querySelector(".site-header");
  function onScroll() { header.classList.toggle("scrolled", window.scrollY > 8); }
  window.addEventListener("scroll", onScroll, { passive: true });
  onScroll();

  var menuBtn = document.getElementById("menuBtn");
  var nav = document.getElementById("nav");
  function setMenu(open) {
    nav.classList.toggle("open", open);
    menuBtn.setAttribute("aria-expanded", String(open));
    menuBtn.setAttribute("aria-label", open ? "Close menu" : "Open menu");
  }
  menuBtn.addEventListener("click", function () { setMenu(!nav.classList.contains("open")); });
  nav.addEventListener("click", function (e) { if (e.target.tagName === "A") setMenu(false); });
  document.addEventListener("keydown", function (e) { if (e.key === "Escape") setMenu(false); });

  // ---------------------------------------------------------------------------
  // Latest release: point download buttons straight at the .dmg
  // ---------------------------------------------------------------------------
  var dlButtons = [document.getElementById("dlHero"), document.getElementById("dlInstall")];
  var verHero = document.getElementById("verHero");

  fetch("https://api.github.com/repos/abdulrasol/sweep/releases/latest", {
    headers: { Accept: "application/vnd.github+json" }
  })
    .then(function (r) { return r.ok ? r.json() : Promise.reject(r.status); })
    .then(function (rel) {
      var assets = rel.assets || [];
      var dmg = assets.find(function (a) { return /universal.*\.dmg$/i.test(a.name); }) ||
                assets.find(function (a) { return /\.dmg$/i.test(a.name); });
      if (dmg) {
        dlButtons.forEach(function (b) { if (b) b.href = dmg.browser_download_url; });
      }
      if (rel.tag_name && verHero) {
        verHero.textContent = "Version " + rel.tag_name.replace(/^v/, "");
      }
    })
    .catch(function () { /* keep the link to the releases page */ });

  // ---------------------------------------------------------------------------
  // Hero readout: rows arrive, then the total counts up (once)
  // ---------------------------------------------------------------------------
  var rows = Array.prototype.slice.call(document.querySelectorAll("#readout li"));
  var totalEl = document.getElementById("readoutTotal");
  var totalBytes = rows.reduce(function (s, li) { return s + Number(li.dataset.bytes || 0); }, 0);

  function fmt(bytes) {
    var gb = bytes / 1073741824;
    return gb.toFixed(1) + " GB";
  }

  if (reduceMotion) {
    rows.forEach(function (li) { li.classList.add("in"); });
    totalEl.textContent = fmt(totalBytes);
  } else {
    totalEl.textContent = "0.0 GB";
    var shown = 0;
    rows.forEach(function (li, i) {
      setTimeout(function () {
        li.classList.add("in");
        var from = shown;
        shown += Number(li.dataset.bytes || 0);
        countTo(from, shown, 260);
      }, 260 + i * 170);
    });
  }

  var raf = null;
  function countTo(from, to, ms) {
    if (raf) cancelAnimationFrame(raf);
    var start = performance.now();
    function step(now) {
      var t = Math.min(1, (now - start) / ms);
      var eased = 1 - Math.pow(1 - t, 3);
      totalEl.textContent = fmt(from + (to - from) * eased);
      if (t < 1) raf = requestAnimationFrame(step);
    }
    raf = requestAnimationFrame(step);
  }

  // ---------------------------------------------------------------------------
  // Catalog of cleanup rules (mirrors src-tauri/src/cleanup_engine)
  // s: S = Safe, R = Review, D = Danger
  // ---------------------------------------------------------------------------
  var CATALOG = [
    // Projects
    { group: "Projects", tool: "Flutter", items: [
      ["build, .dart_tool", "<project>/build", "S"],
      ["CocoaPods (iOS, macOS)", "ios/Pods, macos/Pods", "S"],
      ["Plugin symlinks and lists", "ios/.symlinks, .flutter-plugins*", "S"],
      ["Android Gradle state", "android/.gradle", "S"],
      ["Pub cache (dart pub cache clean)", "~/.pub-cache", "R"]
    ]},
    { group: "Projects", tool: "Node.js", items: [
      ["node_modules", "<project>/node_modules", "S"],
      ["Next.js build", "<project>/.next", "S"],
      ["Tool caches", "<project>/.cache, .pnpm-store", "S"],
      ["dist output", "<project>/dist", "R"]
    ]},
    { group: "Projects", tool: "Rust", items: [
      ["target (with Cargo's CACHEDIR.TAG)", "<project>/target", "S"],
      ["target without the tag", "<project>/target", "R"],
      ["Incremental cache only", "target/{debug,release}/incremental", "S"]
    ]},
    { group: "Projects", tool: "Python", items: [
      ["Bytecode caches", "__pycache__", "S"],
      ["Test and lint caches", ".pytest_cache, .mypy_cache, .ruff_cache, .tox", "S"],
      ["Virtual environments", ".venv, venv (with pyvenv.cfg)", "R"]
    ]},
    { group: "Projects", tool: "Android / Gradle", items: [
      ["Build output", "build, app/build", "S"],
      ["Project Gradle state", "<project>/.gradle", "S"]
    ]},
    { group: "Projects", tool: "PHP / Laravel", items: [
      ["Framework cache and views", "storage/framework/{cache,views}", "S"],
      ["Bootstrap cache, logs", "bootstrap/cache/*.php, storage/logs/*.log", "S"],
      ["vendor", "<project>/vendor", "R"]
    ]},
    { group: "Projects", tool: "Ruby on Rails", items: [
      ["tmp cache, logs", "tmp/cache, log/*.log", "S"],
      ["Bundled gems", "vendor/bundle", "R"]
    ]},
    { group: "Projects", tool: ".NET / C#", items: [
      ["Build binaries", "bin, obj", "S"]
    ]},
    { group: "Projects", tool: "C++ / CMake", items: [
      ["CMake build folders", "build, cmake-build-debug, cmake-build-release", "S"]
    ]},
    { group: "Projects", tool: "Unity", items: [
      ["Library, Temp, Obj, Logs", "<project>/Library …", "D"]
    ]},
    { group: "Projects", tool: "Unreal Engine", items: [
      ["Intermediate, Saved, Binaries, DerivedDataCache", "<project>/Intermediate …", "D"]
    ]},

    // Apple
    { group: "Apple", tool: "Xcode", items: [
      ["DerivedData", "~/Library/Developer/Xcode/DerivedData", "S"],
      ["Previews", "~/Library/Developer/Xcode/UserData/Previews", "S"],
      ["Documentation and app caches", "~/Library/Caches/com.apple.dt.Xcode", "S"],
      ["Old Device Support (iOS, watchOS, tvOS, visionOS)", "~/Library/Developer/Xcode/*DeviceSupport", "S"],
      ["Archives, by day", "~/Library/Developer/Xcode/Archives", "R"]
    ]},
    { group: "Apple", tool: "Simulators", items: [
      ["Simulator caches and logs", "~/Library/Developer/CoreSimulator/Caches", "S"],
      ["Unavailable simulators", "xcrun simctl delete unavailable", "S"],
      ["Simulator runtimes", "xcrun simctl runtime delete", "R"]
    ]},
    { group: "Apple", tool: "CocoaPods", items: [
      ["Download cache", "~/Library/Caches/CocoaPods", "S"],
      ["Spec repos", "~/.cocoapods/repos", "R"]
    ]},
    { group: "Apple", tool: "Device backups", items: [
      ["iPhone and iPad backups", "~/Library/Application Support/MobileSync/Backup", "D"]
    ]},

    // Package caches
    { group: "Packages", tool: "JavaScript", items: [
      ["npm cache (npm cache clean), npx cache", "~/.npm/_cacache, ~/.npm/_npx", "S"],
      ["Yarn (classic and Berry)", "~/Library/Caches/Yarn, ~/.yarn/berry/cache", "S"],
      ["Bun cache", "~/.bun/install/cache", "S"],
      ["pnpm store (pnpm store prune)", "pnpm store path", "S"],
      ["Electron downloads", "~/Library/Caches/electron", "S"],
      ["Playwright, Cypress, Puppeteer browsers", "~/Library/Caches/ms-playwright …", "R"]
    ]},
    { group: "Packages", tool: "Rust", items: [
      ["Cargo registry and git checkouts", "~/.cargo/registry, ~/.cargo/git/checkouts", "S"],
      ["Unused toolchains", "~/.rustup/toolchains", "R"]
    ]},
    { group: "Packages", tool: "Python", items: [
      ["pip, uv, Poetry caches", "~/Library/Caches/pip …", "S"],
      ["Conda packages", "conda clean", "R"]
    ]},
    { group: "Packages", tool: "Go", items: [
      ["Build cache (go clean -cache)", "~/Library/Caches/go-build", "S"],
      ["Module cache (go clean -modcache)", "~/go/pkg/mod", "R"]
    ]},
    { group: "Packages", tool: "Android SDK & Gradle", items: [
      ["Gradle daemon logs, SDK cache", "~/.gradle/daemon, ~/.android/cache", "S"],
      ["Gradle caches", "~/.gradle/caches", "R"],
      ["Emulator images (AVD)", "~/.android/avd", "R"]
    ]},
    { group: "Packages", tool: "PHP", items: [
      ["Composer cache", "~/Library/Caches/composer", "S"]
    ]},
    { group: "Packages", tool: "Homebrew", items: [
      ["Old versions (brew cleanup)", "brew cleanup", "R"]
    ]},
    { group: "Packages", tool: "Docker", items: [
      ["Build cache (docker builder prune)", "docker builder prune", "S"],
      ["Stopped containers, unused images", "docker container / image prune", "R"],
      ["Volumes are never touched", "", "S", true]
    ]},

    // AI tools
    { group: "AI tools", tool: "Local models", items: [
      ["Hugging Face models, datasets, Xet cache", "~/.cache/huggingface", "R"],
      ["Ollama models (ollama rm)", "~/.ollama/models", "R"],
      ["LM Studio models", "~/.lmstudio/models", "R"],
      ["PyTorch cache", "~/.cache/torch", "R"]
    ]},
    { group: "AI tools", tool: "Coding assistants", items: [
      ["Caches, logs, crash dumps", "Claude, Cursor, Codex, Copilot, Cline, Continue, Kiro, Antigravity…", "S"],
      ["Bundled runtimes, old app versions", "e.g. old Claude Code versions", "R"],
      ["Local-only chat history and checkpoints", "Claude Code, Codex, Cline, Aider…", "D"],
      ["Settings, logins, skills and MCP configs are never touched", "", "S", true]
    ]},

    // Editors
    { group: "Editors", tool: "VS Code family & Cursor", items: [
      ["Electron caches", "Cache, Code Cache, GPUCache…", "S"],
      ["Workspace storage of deleted projects", "User/workspaceStorage", "R"],
      ["Old extension versions", "~/.vscode/extensions", "S"]
    ]},
    { group: "Editors", tool: "JetBrains", items: [
      ["Caches and logs of old IDE versions", "~/Library/{Caches,Logs}/JetBrains", "S"],
      ["Data of old IDE versions", "~/Library/Application Support/JetBrains", "R"]
    ]},

    // System
    { group: "System", tool: "macOS", items: [
      ["Your app logs and crash reports", "~/Library/Logs", "S"],
      ["App caches, one by one", "~/Library/Caches/*", "R"],
      ["Trash", "~/.Trash", "R"],
      ["Old files in Downloads", "~/Downloads/*", "R"]
    ]},
    { group: "System", tool: "Apps", items: [
      ["Discord, Spotify caches", "~/Library/…/discord/Cache", "S"],
      ["Telegram media", "Telegram media folders", "R"],
      ["Vagrant boxes", "~/.vagrant.d/boxes", "D"]
    ]}
  ];

  var LEVEL = { S: ["safe", "Safe"], R: ["review", "Review"], D: ["danger", "Danger"] };
  var GROUPS = ["All", "Projects", "Apple", "Packages", "AI tools", "Editors", "System"];
  var SAFETY = [["all", "Any level"], ["S", "Safe"], ["R", "Review"], ["D", "Danger"]];

  var state = { group: "All", safety: "all", q: "" };
  var catalogEl = document.getElementById("catalog");
  var emptyEl = document.getElementById("catalogEmpty");
  var groupChips = document.getElementById("groupChips");
  var safetyChips = document.getElementById("safetyChips");
  var search = document.getElementById("catSearch");

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  function countGroup(g) {
    return CATALOG.filter(function (t) { return g === "All" || t.group === g; })
      .reduce(function (n, t) { return n + t.items.filter(function (i) { return !i[3]; }).length; }, 0);
  }

  function chip(label, pressed, onClick, count) {
    var b = el("button", "chip", label);
    b.type = "button";
    b.setAttribute("aria-pressed", String(pressed));
    if (count != null) { var n = el("span", "n", String(count)); b.appendChild(n); }
    b.addEventListener("click", onClick);
    return b;
  }

  function renderChips() {
    groupChips.textContent = "";
    GROUPS.forEach(function (g) {
      groupChips.appendChild(chip(g, state.group === g, function () { state.group = g; renderChips(); render(); }, countGroup(g)));
    });
    safetyChips.textContent = "";
    SAFETY.forEach(function (s) {
      safetyChips.appendChild(chip(s[1], state.safety === s[0], function () { state.safety = s[0]; renderChips(); render(); }));
    });
  }

  function render() {
    var q = state.q.trim().toLowerCase();
    catalogEl.textContent = "";
    var shown = 0;

    CATALOG.forEach(function (t) {
      if (state.group !== "All" && t.group !== state.group) return;
      var toolMatch = q && (t.tool.toLowerCase().indexOf(q) !== -1 || t.group.toLowerCase().indexOf(q) !== -1);
      var items = t.items.filter(function (i) {
        if (state.safety !== "all" && (i[3] || i[2] !== state.safety)) return false;
        if (!q || toolMatch) return true;
        return (i[0] + " " + i[1]).toLowerCase().indexOf(q) !== -1;
      });
      if (!items.length) return;

      var card = el("article", "tool");
      var head = el("div", "tool-head");
      head.appendChild(el("h3", null, t.tool));
      head.appendChild(el("span", null, t.group));
      card.appendChild(head);

      var ul = el("ul");
      items.forEach(function (i) {
        var li = el("li");
        var what = el("div", "t-what");
        what.appendChild(el("b", null, i[0]));
        if (i[1]) what.appendChild(el("small", null, i[1]));
        li.appendChild(what);
        if (i[3]) {
          li.appendChild(el("span", "tag safe", "Protected"));
        } else {
          li.appendChild(el("span", "tag " + LEVEL[i[2]][0], LEVEL[i[2]][1]));
        }
        ul.appendChild(li);
      });
      card.appendChild(ul);
      catalogEl.appendChild(card);
      shown++;
    });

    emptyEl.hidden = shown !== 0;
  }

  search.addEventListener("input", function () { state.q = search.value; render(); });
  renderChips();
  render();
})();
