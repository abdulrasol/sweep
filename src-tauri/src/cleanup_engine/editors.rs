//! Cleanup rules and collectors for code editors: VS Code family, Cursor, and JetBrains (PLAN 2.11).

use super::{Action, Candidate, Env, GlobalRule, Safety};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use Safety::{Danger, Review, Safe};

const MODULE: &str = "editors";
const GROUP: &str = "Editors";

const VS_CODE_CACHES: [GlobalRule; 9] = crate::cleanup_engine::ai_rules::electron_caches!(
    MODULE,
    "VS Code Cache",
    "Code",
    &["Code", "Visual Studio Code"]
);
const VS_CODE_INSIDERS_CACHES: [GlobalRule; 9] = crate::cleanup_engine::ai_rules::electron_caches!(
    MODULE,
    "VS Code Insiders Cache",
    "Code - Insiders",
    &["Code - Insiders", "Visual Studio Code - Insiders"]
);
const VSCODIUM_CACHES: [GlobalRule; 9] = crate::cleanup_engine::ai_rules::electron_caches!(
    MODULE,
    "VSCodium Cache",
    "VSCodium",
    &["VSCodium"]
);

pub(super) fn editor_rules() -> impl Iterator<Item = &'static GlobalRule> {
    VS_CODE_CACHES
        .iter()
        .chain(VS_CODE_INSIDERS_CACHES.iter())
        .chain(VSCODIUM_CACHES.iter())
}

/// Files and directories under home that must never be deleted, nor any of their ancestors.
pub(super) const PROTECTED: &[&str] = &[
    "Library/Application Support/JetBrains",
    "Library/Application Support/Code/User/settings.json",
    "Library/Application Support/Code/User/keybindings.json",
    "Library/Application Support/Code/User/snippets",
    "Library/Application Support/Code/User/profiles",
    "Library/Application Support/Code/User/globalStorage",
    "Library/Application Support/Code - Insiders/User/settings.json",
    "Library/Application Support/Code - Insiders/User/keybindings.json",
    "Library/Application Support/Code - Insiders/User/snippets",
    "Library/Application Support/Code - Insiders/User/profiles",
    "Library/Application Support/Code - Insiders/User/globalStorage",
    "Library/Application Support/VSCodium/User/settings.json",
    "Library/Application Support/VSCodium/User/keybindings.json",
    "Library/Application Support/VSCodium/User/snippets",
    "Library/Application Support/VSCodium/User/profiles",
    "Library/Application Support/VSCodium/User/globalStorage",
    "Library/Application Support/Cursor/User/settings.json",
    "Library/Application Support/Cursor/User/keybindings.json",
    "Library/Application Support/Cursor/User/snippets",
    "Library/Application Support/Cursor/User/profiles",
];

const WORKSPACE_STORAGE_EDITORS: &[(&str, &str, &[&str])] = &[
    ("Code", "VS Code", &["Code", "Visual Studio Code"]),
    (
        "Code - Insiders",
        "VS Code Insiders",
        &["Code - Insiders", "Visual Studio Code - Insiders"],
    ),
    ("VSCodium", "VSCodium", &["VSCodium"]),
    ("Cursor", "Cursor", &["Cursor"]),
];

pub(super) fn percent_decode(s: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let h1 = iter.next()?;
            let h2 = iter.next()?;
            let d1 = (h1 as char).to_digit(16)? as u8;
            let d2 = (h2 as char).to_digit(16)? as u8;
            bytes.push((d1 << 4) | d2);
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).ok()
}

fn extract_file_uri(val: &serde_json::Value) -> Option<&str> {
    if let serde_json::Value::Object(map) = val {
        for key in &["folder", "workspace"] {
            if let Some(serde_json::Value::String(s)) = map.get(*key) {
                if s.starts_with("file://") {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn collect_orphaned_workspace_storage(env: &Env, out: &mut Vec<Candidate>) {
    for &(app_dir, editor_name, procs) in WORKSPACE_STORAGE_EDITORS {
        let storage_dir = env.home.join(format!(
            "Library/Application Support/{app_dir}/User/workspaceStorage"
        ));
        let Ok(rd) = fs::read_dir(&storage_dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let hash_dir = entry.path();
            if !hash_dir.is_dir() {
                continue;
            }
            let ws_json_path = hash_dir.join("workspace.json");
            let Ok(content) = fs::read_to_string(&ws_json_path) else {
                continue;
            };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
                continue;
            };
            let Some(uri) = extract_file_uri(&val) else {
                continue;
            };
            let raw = &uri["file://".len()..];
            let raw = if let Some(stripped) = raw.strip_prefix("localhost") {
                stripped
            } else {
                raw
            };
            let Some(decoded) = percent_decode(raw) else {
                continue;
            };
            let target = Path::new(&decoded);
            let Some(parent) = target.parent() else {
                continue;
            };
            // Unplugged external drives and permission denial must never look like a deleted project.
            // An entry is an orphan ONLY if its parent directory exists and is readable,
            // and the target itself returns NotFound. Any other outcome means NOT offered.
            if fs::symlink_metadata(parent).is_err() {
                continue;
            }
            match fs::symlink_metadata(target) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => continue,
            }

            let has_chats = fs::symlink_metadata(hash_dir.join("chatSessions")).is_ok();
            let safety = if has_chats { Danger } else { Review };
            let description = if has_chats {
                format!(
                    "Orphaned workspace storage for deleted project {decoded}. Contains Copilot chat sessions."
                )
            } else {
                format!("Orphaned workspace storage for deleted project {decoded}.")
            };

            out.push(Candidate {
                path: hash_dir,
                file_type: format!("{editor_name} Workspace"),
                category: "Workspace Storage".to_string(),
                group: GROUP,
                safety,
                description,
                blocked_if_running: procs.to_vec(),
                project_root: None,
                action: Action::DeletePath,
                known_size: None,
                min_bytes: 1,
                catch_all: false,
            });
        }
    }
}

pub(super) fn parse_jetbrains_dirname(name: &str) -> Option<(&str, (u32, u32))> {
    let alpha_end = name.find(|c: char| !c.is_ascii_alphabetic())?;
    if alpha_end == 0 {
        return None;
    }
    let product = &name[..alpha_end];
    let rest = &name[alpha_end..];
    let dot_pos = rest.find('.')?;
    let year_str = &rest[..dot_pos];
    let minor_str = &rest[dot_pos + 1..];
    if year_str.len() != 4 || !year_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if minor_str.is_empty() || !minor_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year: u32 = year_str.parse().ok()?;
    let minor: u32 = minor_str.parse().ok()?;
    Some((product, (year, minor)))
}

fn collect_jetbrains_root(
    root: &Path,
    safety: Safety,
    category: &'static str,
    desc_kind: &'static str,
    out: &mut Vec<Candidate>,
) {
    let Ok(rd) = fs::read_dir(root) else {
        return;
    };
    struct JbItem {
        path: PathBuf,
        version: (u32, u32),
    }
    let mut by_product: HashMap<String, Vec<JbItem>> = HashMap::new();

    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some((product, version)) = parse_jetbrains_dirname(&name) {
            by_product
                .entry(product.to_string())
                .or_default()
                .push(JbItem { path, version });
        }
    }

    for (product, items) in by_product {
        let Some(max_ver) = items.iter().map(|it| it.version).max() else {
            continue;
        };
        for item in items {
            if item.version < max_ver {
                let (year, minor) = item.version;
                out.push(Candidate {
                    path: item.path,
                    file_type: format!("JetBrains {product}"),
                    category: category.to_string(),
                    group: GROUP,
                    safety,
                    description: format!(
                        "Old {desc_kind} for {product} {year}.{minor}. A newer version is installed."
                    ),
                    blocked_if_running: vec![],
                    project_root: None,
                    action: Action::DeletePath,
                    known_size: None,
                    min_bytes: 1,
                    catch_all: false,
                });
            }
        }
    }
}

fn collect_jetbrains_old_versions(env: &Env, out: &mut Vec<Candidate>) {
    collect_jetbrains_root(
        &env.home.join("Library/Caches/JetBrains"),
        Review,
        "Old Version Cache",
        "cache",
        out,
    );
    collect_jetbrains_root(
        &env.home.join("Library/Logs/JetBrains"),
        Safe,
        "Old Version Logs",
        "log files",
        out,
    );
}

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains(MODULE) {
        return;
    }
    collect_orphaned_workspace_storage(env, out);
    collect_jetbrains_old_versions(env, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jetbrains_dirname_valid() {
        assert_eq!(
            parse_jetbrains_dirname("IntelliJIdea2023.1"),
            Some(("IntelliJIdea", (2023, 1)))
        );
        assert_eq!(
            parse_jetbrains_dirname("PyCharm2022.3"),
            Some(("PyCharm", (2022, 3)))
        );
        assert_eq!(
            parse_jetbrains_dirname("PyCharmCE2024.1"),
            Some(("PyCharmCE", (2024, 1)))
        );
        assert_eq!(
            parse_jetbrains_dirname("RustRover2023.3"),
            Some(("RustRover", (2023, 3)))
        );
    }

    #[test]
    fn parse_jetbrains_dirname_invalid() {
        assert_eq!(parse_jetbrains_dirname("consentOptions"), None);
        assert_eq!(parse_jetbrains_dirname("marketplace"), None);
        assert_eq!(parse_jetbrains_dirname("IntelliJIdea"), None);
        assert_eq!(parse_jetbrains_dirname("2023.1"), None);
        assert_eq!(parse_jetbrains_dirname("IntelliJIdea2023"), None);
        assert_eq!(
            parse_jetbrains_dirname("JetBrainsClient-242.23726.16"),
            None
        );
    }

    #[test]
    fn percent_decode_paths() {
        assert_eq!(
            percent_decode("/Users/test/My%20Projects/app"),
            Some("/Users/test/My Projects/app".to_string())
        );
        assert_eq!(
            percent_decode("/normal/path"),
            Some("/normal/path".to_string())
        );
        assert_eq!(percent_decode("/bad%2"), None);
    }
}
