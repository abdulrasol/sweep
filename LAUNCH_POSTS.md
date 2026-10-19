# منشورات إطلاق Sweep

هذا الملف يجمع منشورات الإطلاق، ووين ينشر كل واحد، وبأي ترتيب.

---

## قبل النشر

انشر بس بعد ما تخلص هذني كلها، لأن كل المنشورات توديهم للموقع:

- [ ] الإصدار `v0.2.0` انبنى وانتشر، يعني مو Draft.
- [ ] على جهاز ثاني، `spctl -a -vvv /Applications/Sweep.app` يطلع `accepted  source=Notarized Developer ID`.
- [ ] الـ branch اندمج بـ `main`، والموقع اشتغل على https://sweep.subultech.top.
- [ ] زر التحميل بالموقع ينزّل الـ `.dmg` مباشرة.
- [ ] بالـ README والموقع صور حقيقية للتطبيق: شاشة الفحص، والنتائج، وشاشة التأكيد. المنشورات اللي بيها صور تاخذ تفاعل أكثر بهواية.
- [ ] الموقع مضاف لـ Google Search Console و Bing Webmaster Tools، وملف `sitemap.xml` مرفوع بالاثنين.

**بعد ما تنشر:** خلك موجود ترد على التعليقات أول ساعتين. هاي الفترة هي اللي تقرر وصول المنشور.

---

## وين ننشر وبأي ترتيب

| الترتيب | المنصة | ليش | ملاحظات |
|---|---|---|---|
| 1 | Reddit r/macapps | أكثر جمهور مباشر لتطبيقات الماك، ويرحبون بالتطبيقات المجانية ومفتوحة المصدر | اقرأ قواعد الـ subreddit واكتب `[Free/Open Source]` بالعنوان |
| 2 | Hacker News (Show HN) | جمهور مطورين قوي، ويهتمون بالمصدر المفتوح وتفاصيل الأمان | الثلاثاء للخميس، بين 4 و6 العصر بتوقيت بغداد. لا تطلب من أحد يسوي upvote |
| 3 | r/FlutterDev و r/iOSProgramming | القصة بدت من `flutter run`، والتطبيق يمسح DerivedData والمحاكيات | لا تنسخ نفس النص. ابدأ من مشكلة المساحة عند مطوري الموبايل |
| 4 | r/rust و r/tauri | مشروع Rust + Tauri | ركّز على الجانب التقني: محرك الأمان، و canonicalize، والأوامر بدون shell |
| 5 | LinkedIn و X | شبكتك والعملاء | انشرهم بنفس اليوم بعد Reddit |
| 6 | مجموعات المطورين العرب (فيسبوك وتيليجرام) | جمهورك المحلي | استخدم النسخة العربية |
| 7 | Product Hunt | لاحقاً | بعد ما تجمع ردود فعل وصور وفيديو قصير |

**قاعدة عامة:** كل subreddit إله قواعد خاصة بالترويج الذاتي. وضّح إنك إنت المطوّر، ولا تنشر نفس النص بأكثر من مكان بنفس الوقت.

---

## 1. Hacker News

**العنوان:**

```
Show HN: Sweep – open-source Mac cleaner for dev caches that won't touch git-tracked files
```

**الرابط:** `https://github.com/abdulrasol/sweep`، أو الموقع.

**النص، كأول تعليق منك:**

```
A flutter run failed on my Mac with "not enough space". I had 1 GB left, buried under years of DerivedData, node_modules, Gradle caches and old simulators.

Sweep scans a projects folder plus the global caches in your home folder and labels each item Safe (regenerates on its own), Review (costs a download or rebuild) or Danger (needs typed confirmation). Nothing is removed without confirmation.

The part I spent the most time on is not deleting the wrong thing:
- any folder inside a project that contains git-tracked files is skipped
- the UI sends item IDs, never paths; the Rust engine re-checks each path against the scan
- symlinks are removed as links, never followed
- where a tool has its own cleanup command (xcrun simctl, brew cleanup, docker prune, go clean, pnpm store prune) Sweep runs that instead
- cleaning waits while Xcode, Android Studio etc. are running
- sizes don't double-count hardlinks or APFS clones, so pnpm's node_modules isn't overstated

Rust + Tauri 2, React UI, MIT licensed, signed and notarized. macOS only for now; the engine has Windows rules but no published build yet.

Site: https://sweep.subultech.top
Code: https://github.com/abdulrasol/sweep

I'd especially like feedback on the rules: anything it should never touch, or caches it misses.
```

---

## 2. Reddit r/macapps

**العنوان:**

```
[Free/Open Source] Sweep – reclaim space from developer caches (Xcode, node_modules, Gradle, simulators) with safety labels on every item
```

**النص:**

```
I built Sweep after my Mac hit 1 GB free during a Flutter build. Years of Xcode DerivedData, node_modules, Gradle caches and old simulator runtimes.

What it does:
- Scans your projects folder + global dev caches (Xcode, CocoaPods, npm/Yarn/pnpm/Bun, Cargo, pip/uv/Poetry, Go, Gradle, Homebrew, Docker, AI tools and local model caches…)
- Labels every item Safe / Review / Danger and tells you what happens if you remove it
- Moves things to the Trash by default
- Never touches folders containing git-tracked files, and uses official commands (simctl, brew cleanup, docker prune) where they exist

Free, MIT licensed, signed and notarized (no xattr tricks needed). Universal build for Apple silicon and Intel.

Download: https://sweep.subultech.top
Source: https://github.com/abdulrasol/sweep

Feedback welcome, especially caches it misses.
```

---

## 3. Reddit r/FlutterDev و r/iOSProgramming

**العنوان:**

```
My Mac ran out of space mid flutter run, so I built an open-source cleaner for mobile dev caches
```

**النص:**

```
DerivedData, old iOS Device Support folders, simulator runtimes, Pods, .dart_tool, Gradle caches… after a few years of mobile work my Mac had 1 GB left and a flutter run failed.

Sweep finds these across all your projects and shows how safe each one is to remove:
- Safe: build, .dart_tool, ios/Pods, DerivedData, old Device Support
- Review: Gradle caches, simulator runtimes (removed with xcrun simctl), pub cache, Xcode Archives
- It waits while Xcode or Android Studio is running, and skips anything with git-tracked files

Free and open source (Rust + Tauri), signed and notarized.
https://sweep.subultech.top

Curious what other caches eat your disk that I should add.
```

---

## 4. Reddit r/rust و r/tauri

**العنوان:**

```
Sweep: a Tauri 2 disk cleaner where the Rust engine, not the UI, decides what can be deleted
```

**النص:**

```
I built a developer disk cleaner with Tauri 2 and wanted the safety rules to live entirely in Rust:

- The frontend only ever sends item IDs. The engine keeps the last scan in State<Mutex<HashMap<..>>>, canonicalizes each path and checks it's under the scan root or a known cache root, against a fixed blocklist.
- symlink_metadata everywhere; links are removed as links.
- Folders containing git-tracked files are never removed.
- External tools (xcrun simctl, brew, docker, go, pnpm, rustup…) run through std::process::Command from an allowlist of enum variants with fixed args, never a shell.
- Rust target/ is only marked Safe when Cargo's CACHEDIR.TAG is present.
- Heavy work runs in spawn_blocking / rayon; sizes are u64 bytes of space actually freed (hardlinks and clones counted once).
- Every rule has a tempfile-based test including a "must not be deleted" case.

MIT licensed: https://github.com/abdulrasol/sweep
Site: https://sweep.subultech.top

Happy to hear how you'd harden it further.
```

---

## 5. LinkedIn

```
A few weeks ago a Flutter build failed on my Mac with "not enough space". The phone was fine. My Mac had 1 GB left.

The culprit was years of developer leftovers: Xcode DerivedData, node_modules in old projects, Gradle caches, simulator runtimes nobody uses anymore.

So I built Sweep, a free, open-source disk cleaner for developers.

✅ Finds build output and caches across 27 tools
✅ Labels every item Safe, Review or Danger
✅ Never touches folders with git-tracked files
✅ Uses official commands (simctl, brew cleanup, docker prune) where they exist
✅ Signed and notarized by Apple

Built with Rust and Tauri, pair-programmed with Gemini, Claude and DeepSeek.

Try it: https://sweep.subultech.top
Source: https://github.com/abdulrasol/sweep

#OpenSource #macOS #Flutter #Rust #DeveloperTools
```

---

## 6. X (Twitter)

```
My Mac had 1 GB free. The culprit: DerivedData, node_modules, Gradle caches, old simulators.

So I built Sweep: a free, open-source Mac cleaner for developers.
• Safe / Review / Danger label on every item
• Never touches git-tracked files
• Signed & notarized

https://sweep.subultech.top
```

حط صورة أو فيديو قصير (GIF) لشاشة النتائج مع التغريدة.

---

## 7. النسخة العربية (فيسبوك وتيليجرام ومجموعات المطورين)

```
بنيت أداة مجانية ومفتوحة المصدر لتنظيف الماك للمطورين 🧹

القصة: مرة فشل عندي flutter run لأن المساحة خلصت. شفت الماك وكان باقي بيه 1 GB بس. السبب سنين من DerivedData و node_modules وكاش Gradle ومحاكيات قديمة.

فسويت Sweep:
• يفحص مجلد مشاريعك والكاشات العامة لأكثر من 25 أداة (Xcode، Flutter، Node، Rust، Gradle، Docker، أدوات الذكاء الاصطناعي…)
• كل عنصر عليه تصنيف: آمن، أو يحتاج مراجعة، أو خطر
• ما يقترب من أي مجلد بيه ملفات متتبعة بـ git
• يستخدم الأوامر الرسمية مثل simctl و brew cleanup و docker prune
• موقّع وموثّق من Apple، ينفتح طبيعي بدون أوامر

مبني بـ Rust و Tauri، ومجاني بالكامل (MIT).

التحميل: https://sweep.subultech.top
الكود: https://github.com/abdulrasol/sweep

جربوه وكولولي شنو الكاشات اللي ما يكشفها 🙏
```

---

## 8. Product Hunt (لاحقاً)

**الاسم:** Sweep

**الشعار القصير (Tagline):**

```
Free, open-source Mac cleaner for developer caches
```

**الوصف:**

```
Sweep finds the gigabytes development leaves behind (node_modules, Xcode DerivedData, Cargo targets, Gradle and package caches, old simulators, AI tool leftovers), labels each one Safe, Review or Danger, and removes only what you confirm. It never touches folders with git-tracked files and uses official cleanup commands where they exist. Signed and notarized, MIT licensed.
```

**تحتاج:** 3 إلى 5 صور بقياس 1270×760، وفيديو أو GIF قصير، وأول تعليق منك يحكي القصة. وأفضل يوم للنشر الثلاثاء أو الأربعاء.
