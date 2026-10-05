//! Old editor extensions and superseded app versions (PLAN 2.11, 2.16).
//!
//! Only two signals are trusted:
//! - the editor's own `.obsolete` list, and
//! - a folder that `extensions.json` no longer references while a newer
//!   version of the same extension is referenced.
//!
//! For versioned folders (e.g. `claude-code/2.1.286`) the highest version is
//! always kept.

use super::{Candidate, Env, Safety};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// (extensions dir relative to home, editor name, editor process names)
const EXTENSION_DIRS: &[(&str, &str, &[&str])] = &[
    (".vscode/extensions", "VS Code", &["Code", "Electron"]),
    (
        ".vscode-insiders/extensions",
        "VS Code Insiders",
        &["Code - Insiders"],
    ),
    (".cursor/extensions", "Cursor", &["Cursor"]),
    (".antigravity/extensions", "Antigravity", &["Antigravity"]),
    (
        ".antigravity-ide/extensions",
        "Antigravity",
        &["Antigravity"],
    ),
    (".trae/extensions", "Trae", &["Trae"]),
    (".trae-cn/extensions", "Trae CN", &["Trae CN"]),
    (".windsurf/extensions", "Windsurf", &["Windsurf"]),
    (".kiro/extensions", "Kiro", &["Kiro"]),
];

/// (folder relative to home whose children are version numbers, label, process names)
const VERSIONED_DIRS: &[(&str, &str, &[&str])] = &[
    (
        "Library/Application Support/Claude/claude-code",
        "Claude Code (bundled)",
        &["Claude"],
    ),
    (
        "Library/Application Support/Claude/claude-code-vm",
        "Claude Code VM (bundled)",
        &["Claude"],
    ),
    (".local/share/claude/versions", "Claude Code", &["claude"]),
];

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if enabled.contains("editors") {
        for (rel, editor, procs) in EXTENSION_DIRS {
            for (path, reason) in old_extensions(&env.home.join(rel)) {
                out.push(Candidate {
                    path,
                    file_type: format!("{editor} extension"),
                    category: "Old Extension".to_string(),
                    safety: Safety::Safe,
                    description: format!("{reason} {editor} does not load it."),
                    blocked_if_running: procs.to_vec(),
                    project_root: None,
                    min_bytes: 1,
                });
            }
        }
    }
    if enabled.contains("ai_assistants") {
        for (rel, label, procs) in VERSIONED_DIRS {
            for path in superseded_versions(&env.home.join(rel)) {
                out.push(Candidate {
                    path,
                    file_type: label.to_string(),
                    category: "Old App Version".to_string(),
                    safety: Safety::Safe,
                    description: format!("An older copy of {label}. A newer version is installed next to it and is kept."),
                    blocked_if_running: procs.to_vec(),
                    project_root: None,
                    min_bytes: 1,
                });
            }
        }
    }
}

/// `publisher.name-1.2.3-darwin-arm64` -> `publisher.name`
pub(super) fn extension_id(folder: &str) -> Option<&str> {
    let bytes = folder.as_bytes();
    (1..bytes.len())
        .find(|&i| bytes[i - 1] == b'-' && bytes[i].is_ascii_digit())
        .map(|i| &folder[..i - 1])
        .filter(|id| id.contains('.'))
}

fn old_extensions(dir: &Path) -> Vec<(PathBuf, &'static str)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let folders: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();

    let obsolete: HashSet<String> = fs::read_to_string(dir.join(".obsolete"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .map(|o| {
            o.into_iter()
                .filter(|(_, v)| v.as_bool() == Some(true))
                .map(|(k, _)| k)
                .collect()
        })
        .unwrap_or_default();

    let referenced: Option<HashSet<String>> = fs::read_to_string(dir.join("extensions.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_array().cloned())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("relativeLocation").and_then(|r| r.as_str()))
                .map(str::to_string)
                .collect()
        });

    let referenced_ids: HashSet<&str> = referenced
        .iter()
        .flatten()
        .filter_map(|r| extension_id(r))
        .collect();

    folders
        .iter()
        .filter_map(|name| {
            // Never touch anything the editor lists as installed.
            if referenced.as_ref().is_some_and(|r| r.contains(name)) {
                return None;
            }
            let reason = if obsolete.contains(name) {
                "Marked obsolete by the editor."
            } else if extension_id(name).is_some_and(|id| referenced_ids.contains(id)) {
                "Replaced by a newer version of the same extension."
            } else {
                return None;
            };
            Some((dir.join(name), reason))
        })
        .collect()
}

fn parse_version(s: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = s.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

fn superseded_versions(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut versions: Vec<(Vec<u64>, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| !t.is_symlink()).unwrap_or(false))
        .filter_map(|e| parse_version(&e.file_name().to_string_lossy()).map(|v| (v, e.path())))
        .collect();
    if versions.len() < 2 {
        return Vec::new();
    }
    versions.sort();
    versions.pop(); // keep the newest
    versions.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_ids() {
        assert_eq!(
            extension_id("anthropic.claude-code-2.1.287-darwin-arm64"),
            Some("anthropic.claude-code")
        );
        assert_eq!(
            extension_id("multi.multi-nightly-0.0.99"),
            Some("multi.multi-nightly")
        );
        assert_eq!(
            extension_id("valentjn.vscode-ltex-13.1.0"),
            Some("valentjn.vscode-ltex")
        );
        assert_eq!(extension_id("extensions.json"), None);
    }

    #[test]
    fn versions_parse() {
        assert!(parse_version("2.1.289") > parse_version("2.1.287"));
        assert!(parse_version("2.1.10") > parse_version("2.1.9"));
        assert_eq!(parse_version("latest"), None);
    }
}
