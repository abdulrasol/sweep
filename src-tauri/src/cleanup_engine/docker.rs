//! Docker cleanup through the docker CLI (PLAN 2.12).
//!
//! Cleans build cache, stopped containers and unused images using official prune commands.
//! Volumes are never touched.

use super::native_commands::{self, DockerKind, NativeCommand};
use super::{Action, Candidate, Env, Safety};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

pub fn parse_docker_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_str, multiplier): (&str, f64) =
        if let Some(n) = s.strip_suffix("TB").or_else(|| s.strip_suffix("tb")) {
            (n, 1_000_000_000_000.0)
        } else if let Some(n) = s.strip_suffix("GB").or_else(|| s.strip_suffix("gb")) {
            (n, 1_000_000_000.0)
        } else if let Some(n) = s.strip_suffix("MB").or_else(|| s.strip_suffix("mb")) {
            (n, 1_000_000.0)
        } else if let Some(n) = s
            .strip_suffix("kB")
            .or_else(|| s.strip_suffix("KB"))
            .or_else(|| s.strip_suffix("kb"))
        {
            (n, 1_000.0)
        } else {
            let n = s.strip_suffix('B').or_else(|| s.strip_suffix('b'))?;
            (n, 1.0)
        };

    let num_str = num_str.trim();
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

fn run_docker_df(docker: &Path, timeout: Duration) -> Option<String> {
    native_commands::run_with_timeout(
        docker,
        &["system", "df", "--format", "{{json .}}"],
        &[],
        timeout,
    )
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
    if !enabled.contains("docker") {
        return;
    }
    let Some(docker) = native_commands::find_tool(&env.home, "docker") else {
        return;
    };
    let Some(stdout) = run_docker_df(&docker, timeout) else {
        return;
    };

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(item_type) = v.get("Type").and_then(|t| t.as_str()) else {
            continue;
        };
        let Some(reclaimable) = v.get("Reclaimable").and_then(|r| r.as_str()) else {
            continue;
        };
        let part = reclaimable.split_whitespace().next().unwrap_or(reclaimable);
        let Some(bytes) = parse_docker_size(part) else {
            continue;
        };
        if bytes == 0 {
            continue;
        }

        let suffix = " Space is freed inside Docker's own disk and the host may release it later.";
        let (path, label, category, safety, desc_base, kind) = match item_type {
            "Build Cache" => (
                PathBuf::from("docker://build-cache"),
                "Docker build cache",
                "Build Cache",
                Safety::Safe,
                "Cached build layers. Rebuilt on the next docker build.",
                DockerKind::BuilderCache,
            ),
            "Containers" => (
                PathBuf::from("docker://containers"),
                "Docker stopped containers",
                "Containers",
                Safety::Review,
                "Stopped containers and their writable layers. Data written inside them is lost.",
                DockerKind::StoppedContainers,
            ),
            "Images" => (
                PathBuf::from("docker://images"),
                "Docker unused images",
                "Images",
                Safety::Review,
                "Images no container uses. Re-downloaded when needed, but images built only on this Mac cannot be downloaded again.",
                DockerKind::UnusedImages,
            ),
            _ => continue, // Local Volumes and unknown types ignored
        };

        out.push(Candidate {
            path,
            file_type: label.to_string(),
            category: category.to_string(),
            safety,
            description: format!("{desc_base}{suffix}"),
            blocked_if_running: vec![],
            project_root: None,
            action: Action::Command(NativeCommand::DockerPrune {
                docker: docker.clone(),
                kind,
            }),
            known_size: Some(bytes),
            min_bytes: 1,
            group: "Dev Tools",
            catch_all: false,
        });
    }
}
