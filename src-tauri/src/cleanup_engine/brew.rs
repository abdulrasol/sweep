//! Homebrew old versions cleanup through `brew cleanup` (PLAN 2.13).
//!
//! Cleans old versions of installed formulae and casks using Homebrew's official command.
//! Active installed versions are never removed. Downloads older than 120 days are also cleaned.
//! Volumes and system paths are never touched.

use super::native_commands::{self, NativeCommand};
use super::{Action, Candidate, Env, Safety};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

const BREW_ENVS: [(&str, &str); 3] = [
    ("HOMEBREW_NO_AUTO_UPDATE", "1"),
    ("HOMEBREW_NO_ANALYTICS", "1"),
    ("HOMEBREW_NO_ENV_HINTS", "1"),
];

/// Parse sizes like "61.2MB", "4kB", "4KB", "0B", "2GB".
/// Uses decimal 1000-based multipliers, case-insensitive.
pub fn parse_brew_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    let (num_part, multiplier) = if let Some(n) = lower.strip_suffix("tb") {
        (&s[..n.len()], 1_000_000_000_000.0)
    } else if let Some(n) = lower.strip_suffix("gb") {
        (&s[..n.len()], 1_000_000_000.0)
    } else if let Some(n) = lower.strip_suffix("mb") {
        (&s[..n.len()], 1_000_000.0)
    } else if let Some(n) = lower.strip_suffix("kb") {
        (&s[..n.len()], 1_000.0)
    } else {
        let n = lower.strip_suffix('b')?;
        (&s[..n.len()], 1.0)
    };

    let num_str = num_part.trim().replace(',', "");
    if num_str.is_empty() {
        return None;
    }
    let val: f64 = num_str.parse().ok()?;
    if val < 0.0 || !val.is_finite() {
        return None;
    }
    let bytes = (val * multiplier).round();
    if bytes > u64::MAX as f64 {
        return None;
    }
    Some(bytes as u64)
}

/// Parse a "Would remove: <path> (<files>, <size>)" line.
/// Returns (path, bytes) only if the path contains "/Cellar/" or "/Caskroom/".
pub fn parse_cleanup_line(line: &str) -> Option<(&str, u64)> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("Would remove:")?.trim();
    let paren_open = rest.rfind(" (")?;
    let path = rest[..paren_open].trim();
    if !path.contains("/Cellar/") && !path.contains("/Caskroom/") {
        return None;
    }
    let inside = rest[paren_open + 2..].trim_end().strip_suffix(')')?.trim();
    let size_part = inside.split(',').next_back()?.trim();
    let bytes = parse_brew_size(size_part)?;
    Some((path, bytes))
}

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    collect_with_timeout(env, enabled, DEFAULT_TIMEOUT, out);
}

pub(crate) fn collect_with_timeout(
    env: &Env,
    enabled: &HashSet<&str>,
    timeout: Duration,
    out: &mut Vec<Candidate>,
) {
    if !enabled.contains("homebrew") {
        return;
    }
    let Some(brew) = native_commands::find_tool(&env.home, "brew") else {
        return;
    };

    let Some(stdout) =
        native_commands::run_with_timeout(&brew, &["cleanup", "-n"], &BREW_ENVS, timeout)
    else {
        return;
    };

    let mut count = 0usize;
    let mut total_bytes = 0u64;

    for line in stdout.lines() {
        if let Some((_path, bytes)) = parse_cleanup_line(line) {
            count += 1;
            total_bytes = total_bytes.saturating_add(bytes);
        }
    }

    if count == 0 || total_bytes == 0 {
        return;
    }

    out.push(Candidate {
        path: PathBuf::from("brew://old-versions"),
        file_type: "Homebrew old versions".to_string(),
        category: "Old Versions".to_string(),
        safety: Safety::Review,
        description: format!(
            "{count} old versions of installed formulae and casks that a newer installed version replaced (brew cleanup). Installed versions are never removed. Downloads older than 120 days are removed too. Size is an estimate from brew."
        ),
        blocked_if_running: vec!["brew"],
        project_root: None,
        action: Action::Command(NativeCommand::BrewCleanup { brew }),
        known_size: Some(total_bytes),
        min_bytes: 1,
        group: "Dev Tools",
        module: "homebrew",
        catch_all: false,
    });
}
