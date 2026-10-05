//! Cleanup rules for AI assistants and coding agents (PLAN 2.16).
//!
//! Three tiers, never mixed in one rule:
//! - caches, logs and crash dumps: SAFE, rebuilt by the app;
//! - big re-downloadable runtimes and local mirrors of server-side chats: REVIEW;
//! - local-only chat history, transcripts and checkpoints: DANGER (typed confirmation).
//!
//! Settings, login tokens, instructions, skills and MCP configs are never targets;
//! their roots are also in `protected_paths`.
//!
//! Sources: each tool's docs or well-documented community findings, collected in
//! PLAN.md 2.16. Paths that do not exist are ignored, so a rule for an app the user
//! does not have costs nothing.

use super::{GlobalRule, Safety, MIN_SPLIT_CHILD_BYTES};
use Safety::{Danger, Review, Safe};

const MODULE: &str = "ai_assistants";

const fn cache(
    label: &'static str,
    home_rel: &'static str,
    blocked: &'static [&'static str],
) -> GlobalRule {
    GlobalRule {
        module: MODULE,
        label,
        home_rel,
        category: "AI App Cache",
        safety: Safe,
        blocked_if_running: blocked,
        split_children: false,
        note: "",
        min_bytes: 1,
    }
}

const fn history(
    label: &'static str,
    home_rel: &'static str,
    blocked: &'static [&'static str],
    split_children: bool,
    note: &'static str,
) -> GlobalRule {
    GlobalRule {
        module: MODULE,
        label,
        home_rel,
        category: "AI Chat History",
        safety: Danger,
        blocked_if_running: blocked,
        split_children,
        note,
        min_bytes: if split_children {
            MIN_SPLIT_CHILD_BYTES / 10
        } else {
            1
        },
    }
}

const fn review(
    label: &'static str,
    home_rel: &'static str,
    category: &'static str,
    blocked: &'static [&'static str],
    note: &'static str,
) -> GlobalRule {
    GlobalRule {
        module: MODULE,
        label,
        home_rel,
        category,
        safety: Review,
        blocked_if_running: blocked,
        split_children: false,
        note,
        min_bytes: 1,
    }
}

macro_rules! electron_caches {
    ($label:expr, $dir:literal, $blocked:expr) => {
        [
            cache(
                $label,
                concat!("Library/Application Support/", $dir, "/Cache"),
                $blocked,
            ),
            cache(
                $label,
                concat!("Library/Application Support/", $dir, "/Code Cache"),
                $blocked,
            ),
            cache(
                $label,
                concat!("Library/Application Support/", $dir, "/GPUCache"),
                $blocked,
            ),
            cache(
                $label,
                concat!("Library/Application Support/", $dir, "/DawnGraphiteCache"),
                $blocked,
            ),
            cache(
                $label,
                concat!("Library/Application Support/", $dir, "/DawnWebGPUCache"),
                $blocked,
            ),
            cache(
                $label,
                concat!("Library/Application Support/", $dir, "/CachedData"),
                $blocked,
            ),
            cache(
                $label,
                concat!(
                    "Library/Application Support/",
                    $dir,
                    "/CachedExtensionVSIXs"
                ),
                $blocked,
            ),
            cache(
                $label,
                concat!("Library/Application Support/", $dir, "/Crashpad"),
                $blocked,
            ),
            cache(
                $label,
                concat!("Library/Application Support/", $dir, "/logs"),
                $blocked,
            ),
        ]
    };
}

const CLAUDE: &[&str] = &["Claude"];
const CHATGPT: &[&str] = &["ChatGPT"];
const CURSOR: &[&str] = &["Cursor"];
const ANTIGRAVITY: &[&str] = &["Antigravity"];
const NONE: &[&str] = &[];

const CLAUDE_DESKTOP: [GlobalRule; 9] = electron_caches!("Claude Desktop", "Claude", CLAUDE);
const CODEX_APP: [GlobalRule; 9] = electron_caches!("Codex App", "Codex", &["Codex"]);
const CURSOR_APP: [GlobalRule; 9] = electron_caches!("Cursor", "Cursor", CURSOR);
const ANTIGRAVITY_APP: [GlobalRule; 9] =
    electron_caches!("Antigravity", "Antigravity", ANTIGRAVITY);
const WINDSURF_APP: [GlobalRule; 9] = electron_caches!("Windsurf", "Windsurf", &["Windsurf"]);
const TRAE_APP: [GlobalRule; 9] = electron_caches!("Trae", "Trae", &["Trae"]);
const TRAE_CN_APP: [GlobalRule; 9] = electron_caches!("Trae CN", "Trae CN", &["Trae CN"]);
const KIRO_APP: [GlobalRule; 9] = electron_caches!("Kiro", "Kiro", &["Kiro"]);

const OTHERS: &[GlobalRule] = &[
    // ----- Claude Desktop ---------------------------------------------------
    review(
        "Claude Desktop VM image",
        "Library/Application Support/Claude/vm_bundles/claudevm.bundle/rootfs.img",
        "AI Runtime",
        CLAUDE,
        "Disk image of the Linux VM used by Claude's agent mode (often 10 GB+). Claude downloads it again when needed. Your agent session data (sessiondata.img) is kept.",
    ),
    cache("Claude Desktop", "Library/Caches/com.anthropic.claudefordesktop", CLAUDE),
    cache("Claude Desktop", "Library/Caches/com.anthropic.claudefordesktop.ShipIt", CLAUDE),
    cache("Claude Desktop", "Library/Logs/Claude", CLAUDE),
    // ----- Claude Code CLI --------------------------------------------------
    history(
        "Claude Code transcripts",
        ".claude/projects",
        &["claude"],
        true,
        "Claude Code conversation transcripts for one project. Deleting them removes these sessions from /resume. Claude Code already deletes transcripts older than cleanupPeriodDays (default 30).",
    ),
    review(
        "Claude Code checkpoints",
        ".claude/file-history",
        "AI Chat History",
        &["claude"],
        "File snapshots used by Claude Code's rewind. Deleting them means past sessions can no longer be rewound.",
    ),
    cache("Claude Code", ".claude/shell-snapshots", &["claude"]),
    cache("Claude Code", ".claude/debug", &["claude"]),
    cache("Claude Code", ".claude/statsig", &["claude"]),
    cache("Claude Code", ".claude/todos", &["claude"]),
    // ----- ChatGPT ----------------------------------------------------------
    cache("ChatGPT", "Library/Caches/com.openai.chat", CHATGPT),
    cache("ChatGPT", "Library/Caches/ChatGPTHelper", CHATGPT),
    review(
        "ChatGPT local chat copy",
        "Library/Application Support/com.openai.chat/conversations-v3-*",
        "AI Chat Mirror",
        CHATGPT,
        "Local copy of your ChatGPT conversations. The chats stay in your OpenAI account and download again when you open them.",
    ),
    // ----- Codex ------------------------------------------------------------
    history(
        "Codex sessions",
        ".codex/sessions",
        &["codex", "Codex"],
        true,
        "Codex conversation transcripts for one year. Deleting them removes these sessions from codex resume.",
    ),
    history(
        "Codex archived sessions",
        ".codex/archived_sessions",
        &["codex", "Codex"],
        false,
        "Codex sessions you archived. Deleting them is permanent.",
    ),
    review(
        "Codex prompt history",
        ".codex/history.jsonl",
        "AI Chat History",
        &["codex", "Codex"],
        "The list of prompts you typed in Codex, used for up-arrow recall.",
    ),
    cache("Codex", ".codex/log", &["codex", "Codex"]),
    cache("Codex", "Library/Logs/Codex", &["Codex"]),
    // ----- Cursor -----------------------------------------------------------
    cache("Cursor", "Library/Caches/com.todesktop.230313mzl4w4u92", CURSOR),
    cache("Cursor", "Library/Caches/com.todesktop.230313mzl4w4u92.ShipIt", CURSOR),
    history(
        "Cursor CLI chats",
        ".cursor/chats",
        &["cursor-agent", "Cursor"],
        false,
        "Chats from the Cursor CLI agent. Chats inside the Cursor editor are stored elsewhere and are not touched.",
    ),
    history(
        "Cursor agent transcripts",
        ".cursor/projects/*/agent-transcripts",
        CURSOR,
        false,
        "Agent transcripts for one project. Cursor also keeps chats in its own database, so some history may remain visible.",
    ),
    // ----- Google Antigravity -------------------------------------------------
    history(
        "Antigravity conversations",
        ".gemini/antigravity/brain",
        ANTIGRAVITY,
        true,
        "One Antigravity conversation: its plans, task lists, walkthroughs and screenshots. Deleting it is permanent.",
    ),
    history(
        "Antigravity conversations (old install)",
        ".gemini/antigravity-ide/brain",
        ANTIGRAVITY,
        false,
        "Conversations left by an older Antigravity install. Newer versions keep their own copy under .gemini/antigravity.",
    ),
    history(
        "Antigravity backup",
        ".gemini/antigravity-backup",
        ANTIGRAVITY,
        false,
        "A backup copy of Antigravity conversations made during an upgrade.",
    ),
    history(
        "Antigravity conversations (legacy)",
        ".gemini/antigravity/conversations",
        ANTIGRAVITY,
        false,
        "Conversation files from earlier Antigravity versions.",
    ),
    review(
        "Antigravity browser recordings",
        ".gemini/antigravity/browser_recordings",
        "AI Media",
        ANTIGRAVITY,
        "Screen recordings the Antigravity browser agent captured while testing your apps.",
    ),
    history(
        "Antigravity CLI transcripts",
        ".gemini/antigravity-cli/brain",
        &["antigravity", "agy"],
        true,
        "One Antigravity CLI conversation transcript. Deleting it is permanent.",
    ),
    // ----- Gemini CLI / Qwen Code ---------------------------------------------
    history(
        "Gemini CLI project data",
        ".gemini/tmp",
        &["gemini"],
        true,
        "Gemini CLI chats, checkpoints and logs for one project. Gemini CLI also deletes sessions older than its retention setting (default 30 days).",
    ),
    history(
        "Qwen Code project data",
        ".qwen/tmp",
        &["qwen"],
        true,
        "Qwen Code chats, checkpoints and logs for one project.",
    ),
    // ----- GitHub Copilot ----------------------------------------------------
    cache("Copilot CLI", ".copilot/logs", &["copilot"]),
    history(
        "Copilot CLI sessions",
        ".copilot/session-state",
        &["copilot"],
        false,
        "Copilot CLI session history. Deleting it means these sessions can no longer be resumed.",
    ),
    history(
        "Copilot Chat (VS Code)",
        "Library/Application Support/Code/User/workspaceStorage/*/chatSessions",
        &["Code", "Electron"],
        false,
        "Copilot Chat history for one VS Code workspace.",
    ),
    // ----- Cline / Roo Code / Kilo Code (any VS Code-based editor) ------------
    review(
        "Cline checkpoints",
        "Library/Application Support/*/User/globalStorage/saoudrizwan.claude-dev/checkpoints",
        "AI Checkpoints",
        NONE,
        "Shadow git repositories Cline uses to restore files. They can grow to tens of GB. Deleting them disables 'Restore' on past tasks.",
    ),
    history(
        "Cline tasks",
        "Library/Application Support/*/User/globalStorage/saoudrizwan.claude-dev/tasks",
        NONE,
        false,
        "Cline task conversations. Deleting them clears Cline's task history.",
    ),
    cache(
        "Cline browser",
        "Library/Application Support/*/User/globalStorage/saoudrizwan.claude-dev/puppeteer",
        NONE,
    ),
    review(
        "Roo Code checkpoints",
        "Library/Application Support/*/User/globalStorage/rooveterinaryinc.roo-cline/checkpoints",
        "AI Checkpoints",
        NONE,
        "Shadow git repositories Roo Code uses to restore files.",
    ),
    history(
        "Roo Code tasks",
        "Library/Application Support/*/User/globalStorage/rooveterinaryinc.roo-cline/tasks",
        NONE,
        false,
        "Roo Code task conversations.",
    ),
    review(
        "Kilo Code checkpoints",
        "Library/Application Support/*/User/globalStorage/kilocode.kilo-code/checkpoints",
        "AI Checkpoints",
        NONE,
        "Shadow git repositories Kilo Code uses to restore files.",
    ),
    history(
        "Kilo Code tasks",
        "Library/Application Support/*/User/globalStorage/kilocode.kilo-code/tasks",
        NONE,
        false,
        "Kilo Code task conversations.",
    ),
    // ----- Continue -----------------------------------------------------------
    history(
        "Continue sessions",
        ".continue/sessions",
        NONE,
        false,
        "Continue chat sessions.",
    ),
    review(
        "Continue index",
        ".continue/index",
        "AI Index",
        NONE,
        "Codebase embeddings Continue uses for search. Rebuilt automatically, which takes a while on large projects.",
    ),
    cache("Continue", ".continue/logs", NONE),
    // ----- DeepSeek CLI (layout seen on the user's Mac, 2026-10-05) -----------
    history(
        "DeepSeek sessions",
        ".deepseek/sessions",
        &["deepseek"],
        false,
        "DeepSeek CLI conversation sessions. Deleting them is permanent.",
    ),
    review(
        "DeepSeek prompt history",
        ".deepseek/composer_history.txt",
        "AI Chat History",
        &["deepseek"],
        "The list of prompts you typed in DeepSeek CLI.",
    ),
    cache("DeepSeek", ".deepseek/audit.log", &["deepseek"]),
    // ----- Windsurf / Amp / opencode / Aider -----------------------------------
    history(
        "Windsurf Cascade",
        ".codeium/windsurf/cascade",
        &["Windsurf"],
        false,
        "Windsurf Cascade conversations.",
    ),
    history(
        "Amp threads",
        ".local/share/amp/threads",
        &["amp"],
        false,
        "Local copies of Amp threads.",
    ),
    cache("Amp", ".cache/amp/logs", &["amp"]),
    cache("opencode", ".local/share/opencode/log", &["opencode"]),
    cache("Aider", ".aider/caches", &["aider"]),
];

/// Every AI assistant rule, in display order.
pub(super) fn ai_rules() -> impl Iterator<Item = &'static GlobalRule> {
    static GROUPS: [&[GlobalRule]; 9] = [
        &CLAUDE_DESKTOP,
        &CODEX_APP,
        &CURSOR_APP,
        &ANTIGRAVITY_APP,
        &WINDSURF_APP,
        &TRAE_APP,
        &TRAE_CN_APP,
        &KIRO_APP,
        OTHERS,
    ];
    GROUPS.iter().flat_map(|g| g.iter())
}

/// Files and folders that must never be deleted, nor any of their ancestors.
pub(super) const PROTECTED: &[&str] = &[
    ".claude",
    ".claude.json",
    ".claude/settings.json",
    ".claude/CLAUDE.md",
    ".claude/.credentials.json",
    ".codex",
    ".codex/auth.json",
    ".codex/config.toml",
    ".codex/AGENTS.md",
    ".gemini",
    ".gemini/settings.json",
    ".gemini/oauth_creds.json",
    ".gemini/GEMINI.md",
    ".gemini/antigravity",
    ".qwen",
    ".qwen/settings.json",
    ".qwen/oauth_creds.json",
    ".cursor",
    ".copilot",
    ".copilot/config.json",
    ".copilot/settings.json",
    ".continue",
    ".continue/config.yaml",
    ".continue/config.json",
    ".cline",
    ".deepseek",
    ".deepseek/config.toml",
    ".deepseek/instructions.md",
    ".deepseek/skills",
    ".deepseek/automations",
    ".deepseek/tasks",
    ".kimi-work",
    ".kimi-work/bin",
    ".codeium",
    ".local/share",
    ".local/share/amp/secrets.json",
    "Library/Application Support/Claude",
    "Library/Application Support/Claude/vm_bundles/claudevm.bundle/sessiondata.img",
    "Library/Application Support/Claude/claude_desktop_config.json",
    "Library/Application Support/com.openai.chat",
    "Library/Application Support/Cursor/User/globalStorage/state.vscdb",
    "Library/Application Support/Code/User/settings.json",
];
