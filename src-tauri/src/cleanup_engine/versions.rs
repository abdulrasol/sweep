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

/// A folder whose children are versions of the same thing; the newest `keep` stay.
struct VersionedDir {
    module: &'static str,
    group: &'static str,
    home_rel: &'static str,
    label: &'static str,
    category: &'static str,
    keep: usize,
    blocked_if_running: &'static [&'static str],
    description: &'static str,
}

const VERSIONED_DIRS: &[VersionedDir] = &[
    VersionedDir {
        module: "ai_assistants",
        group: "AI Tools",
        home_rel: "Library/Application Support/Claude/claude-code",
        label: "Claude Code (bundled)",
        category: "Old App Version",
        keep: 1,
        blocked_if_running: &["Claude"],
        description: "An older copy of Claude Code bundled with Claude Desktop. The newest copy is kept.",
    },
    VersionedDir {
        module: "ai_assistants",
        group: "AI Tools",
        home_rel: "Library/Application Support/Claude/claude-code-vm",
        label: "Claude Code VM (bundled)",
        category: "Old App Version",
        keep: 1,
        blocked_if_running: &["Claude"],
        description: "An older copy of Claude Code for the agent VM. The newest copy is kept.",
    },
    VersionedDir {
        module: "ai_assistants",
        group: "AI Tools",
        home_rel: ".local/share/claude/versions",
        label: "Claude Code",
        category: "Old App Version",
        keep: 1,
        blocked_if_running: &["claude"],
        description: "An older Claude Code binary left by an update. The newest is kept.",
    },
    // Xcode copies debug symbols for every OS version a device was connected with
    // (PLAN 2.1). Keep the two newest per platform; older ones come back if a device
    // on that version is connected again.
    VersionedDir {
        module: "xcode",
        group: "Dev Tools",
        home_rel: "Library/Developer/Xcode/iOS DeviceSupport",
        label: "iOS Device Support",
        category: "Device Support",
        keep: 2,
        blocked_if_running: &["Xcode"],
        description: "Debug symbols for an older iOS version. Xcode copies them again if you connect a device running this version.",
    },
    VersionedDir {
        module: "xcode",
        group: "Dev Tools",
        home_rel: "Library/Developer/Xcode/watchOS DeviceSupport",
        label: "watchOS Device Support",
        category: "Device Support",
        keep: 2,
        blocked_if_running: &["Xcode"],
        description: "Debug symbols for an older watchOS version. Xcode copies them again when needed.",
    },
    VersionedDir {
        module: "xcode",
        group: "Dev Tools",
        home_rel: "Library/Developer/Xcode/tvOS DeviceSupport",
        label: "tvOS Device Support",
        category: "Device Support",
        keep: 2,
        blocked_if_running: &["Xcode"],
        description: "Debug symbols for an older tvOS version. Xcode copies them again when needed.",
    },
    VersionedDir {
        module: "xcode",
        group: "Dev Tools",
        home_rel: "Library/Developer/Xcode/visionOS DeviceSupport",
        label: "visionOS Device Support",
        category: "Device Support",
        keep: 2,
        blocked_if_running: &["Xcode"],
        description: "Debug symbols for an older visionOS version. Xcode copies them again when needed.",
    },
];

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if enabled.contains("editors") {
        for (rel, editor, procs) in EXTENSION_DIRS {
            for (path, reason) in old_extensions(&env.home.join(rel)) {
                out.push(Candidate {
                    path,
                    file_type: format!("{editor} extension"),
                    category: "Old Extension".to_string(),
                    group: "Editors",
                    safety: Safety::Safe,
                    description: format!("{reason} {editor} does not load it."),
                    blocked_if_running: procs.to_vec(),
                    project_root: None,
                    action: super::Action::DeletePath,
                    known_size: None,
                    min_bytes: 1,
                    catch_all: false,
                });
            }
        }
    }
    for dir in VERSIONED_DIRS.iter().filter(|d| enabled.contains(d.module)) {
        for path in superseded_versions(&env.home.join(dir.home_rel), dir.keep) {
            out.push(Candidate {
                path,
                file_type: dir.label.to_string(),
                category: dir.category.to_string(),
                group: dir.group,
                safety: Safety::Safe,
                description: dir.description.to_string(),
                blocked_if_running: dir.blocked_if_running.to_vec(),
                project_root: None,
                action: super::Action::DeletePath,
                known_size: None,
                min_bytes: 1,
                catch_all: false,
            });
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

/// First dotted number in a folder name: "iPhone15,2 18.1 (22B83)" -> [18, 1],
/// "16.4.1 (20E252)" -> [16, 4, 1], "2.1.286" -> [2, 1, 286].
fn parse_version(s: &str) -> Option<Vec<u64>> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter(|tok| tok.contains('.'))
        .find_map(|tok| {
            let parts: Option<Vec<u64>> = tok.split('.').map(|p| p.parse().ok()).collect();
            parts.filter(|p| p.len() >= 2)
        })
}

fn superseded_versions(dir: &Path, keep: usize) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut versions: Vec<(Vec<u64>, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| parse_version(&e.file_name().to_string_lossy()).map(|v| (v, e.path())))
        .collect();
    if versions.len() <= keep {
        return Vec::new();
    }
    versions.sort();
    versions.truncate(versions.len() - keep); // drop the newest `keep` from the list
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
        assert_eq!(parse_version("iPhone15,2 18.1 (22B83)"), Some(vec![18, 1]));
        assert_eq!(parse_version("16.4.1 (20E252)"), Some(vec![16, 4, 1]));
    }
}
