//! iOS Simulator devices and runtimes, removed through `xcrun simctl` (PLAN 2.2).
//!
//! Safety rules for runtimes (learned from a real bug in another cleaner, which
//! matched devices by `platformIdentifier` and deleted every runtime):
//! - devices are matched to a runtime by `runtimeIdentifier`, the key that
//!   `simctl list devices -j` uses;
//! - a runtime with any device, a runtime we cannot match, a runtime that is not
//!   `deletable` or not in the `Ready` state is never offered;
//! - the newest runtime of each platform is always kept.

use super::{Action, Candidate, Env, NativeCommand, Safety};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const XCRUN: &str = "/usr/bin/xcrun";

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains("xcode") || !cfg!(target_os = "macos") || !developer_tools_installed() {
        return;
    }
    let devices = xcrun_json(&["simctl", "list", "devices", "-j"]);
    let runtimes = xcrun_json(&["simctl", "runtime", "list", "-j"]);

    if let Some(devices) = &devices {
        let udids = unavailable_devices(devices);
        if !udids.is_empty() {
            let base = env.home.join("Library/Developer/CoreSimulator/Devices");
            let size: u64 = udids.iter().map(|u| super::disk_usage(&base.join(u))).sum();
            out.push(command_candidate(
                base,
                "Unavailable Simulators",
                Safety::Safe,
                format!(
                    "{} simulator devices whose iOS runtime is no longer installed. They cannot run anymore.",
                    udids.len()
                ),
                vec!["simctl".into(), "delete".into(), "unavailable".into()],
                size,
            ));
        }
    }

    if let (Some(devices), Some(runtimes)) = (&devices, &runtimes) {
        for rt in removable_runtimes(runtimes, devices) {
            out.push(command_candidate(
                PathBuf::from(&rt.path),
                "Simulator Runtime",
                Safety::Review,
                format!(
                    "{} simulator runtime. No simulator uses it and a newer {} runtime is installed. Xcode can download it again (several GB).",
                    rt.label, rt.platform
                ),
                vec!["simctl".into(), "runtime".into(), "delete".into(), rt.identifier.clone()],
                rt.size,
            ));
        }
    }
}

fn command_candidate(
    path: PathBuf,
    label: &str,
    safety: Safety,
    description: String,
    args: Vec<String>,
    size: u64,
) -> Candidate {
    Candidate {
        path,
        file_type: label.to_string(),
        category: "Simulators".to_string(),
        safety,
        description,
        blocked_if_running: vec!["Simulator", "Xcode"],
        project_root: None,
        action: Action::Command(NativeCommand::Xcrun(args)),
        known_size: Some(size),
        min_bytes: 1,
        group: "Dev Tools",
        catch_all: false,
    }
}

/// `xcode-select -p` never opens the "install developer tools" dialog, unlike the
/// `/usr/bin/xcrun` shim, so check it first.
fn developer_tools_installed() -> bool {
    std::process::Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn xcrun_json(args: &[&str]) -> Option<Value> {
    let out = std::process::Command::new(XCRUN).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// UDIDs of devices whose runtime is gone (`isAvailable: false`).
pub(super) fn unavailable_devices(devices: &Value) -> Vec<String> {
    devices
        .get("devices")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|by_runtime| by_runtime.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter(|d| d.get("isAvailable").and_then(Value::as_bool) == Some(false))
        .filter_map(|d| d.get("udid").and_then(Value::as_str))
        .filter(|u| is_uuid(u))
        .map(str::to_string)
        .collect()
}

#[derive(Debug, PartialEq)]
pub(super) struct Runtime {
    pub identifier: String,
    pub label: String,
    pub platform: String,
    pub path: String,
    pub size: u64,
}

pub(super) fn removable_runtimes(runtimes: &Value, devices: &Value) -> Vec<Runtime> {
    let used: HashSet<&str> = devices
        .get("devices")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter(|(_, list)| list.as_array().is_some_and(|l| !l.is_empty()))
                .map(|(k, _)| k.as_str())
                .collect()
        })
        .unwrap_or_default();

    struct Entry<'a> {
        v: &'a Value,
        version: Vec<u64>,
        platform: &'a str,
    }
    let entries: Vec<Entry> = runtimes
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(|v| {
            Some(Entry {
                version: v
                    .get("version")?
                    .as_str()?
                    .split('.')
                    .map(|p| p.parse().unwrap_or(0))
                    .collect(),
                platform: v.get("platformIdentifier")?.as_str()?,
                v,
            })
        })
        .collect();

    let mut newest: HashMap<&str, &Vec<u64>> = HashMap::new();
    for e in &entries {
        let best = newest.entry(e.platform).or_insert(&e.version);
        if e.version > **best {
            *best = &e.version;
        }
    }

    entries
        .iter()
        .filter(|e| newest.get(e.platform).is_some_and(|n| **n != e.version))
        .filter_map(|e| {
            let v = e.v;
            let identifier = v.get("identifier")?.as_str()?;
            let runtime_id = v.get("runtimeIdentifier")?.as_str()?;
            let deletable = v.get("deletable").and_then(Value::as_bool) == Some(true);
            let ready = v
                .get("state")
                .and_then(Value::as_str)
                .is_some_and(|s| s.eq_ignore_ascii_case("ready"));
            if !deletable || !ready || used.contains(runtime_id) || !is_uuid(identifier) {
                return None;
            }
            let os = runtime_id
                .rsplit('.')
                .next()
                .and_then(|s| s.split('-').next())
                .unwrap_or("Simulator");
            let version = v.get("version").and_then(Value::as_str).unwrap_or("?");
            let build = v.get("build").and_then(Value::as_str).unwrap_or("");
            Some(Runtime {
                identifier: identifier.to_string(),
                label: format!("{os} {version} ({build})"),
                platform: os.to_string(),
                path: v
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("/Library/Developer/CoreSimulator/Images")
                    .to_string(),
                size: v.get("sizeBytes").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect()
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.chars().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// The only commands Sweep will ever run.
pub(super) fn is_allowed_command(args: &[String]) -> bool {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["simctl", "delete", "unavailable"] => true,
        ["simctl", "runtime", "delete", id] => is_uuid(id),
        _ => false,
    }
}

pub(super) fn run_command(args: &[String]) -> Result<(), String> {
    if !is_allowed_command(args) {
        return Err("Sweep refused to run an unexpected command.".to_string());
    }
    let out = std::process::Command::new(XCRUN)
        .args(args)
        .output()
        .map_err(|e| format!("Could not run xcrun: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "xcrun {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const A: &str = "11111111-2222-3333-4444-555555555555";
    const B: &str = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE";
    const C: &str = "99999999-8888-7777-6666-555555555555";
    const D: &str = "12345678-1234-1234-1234-123456789ABC";

    fn runtime(id: &str, rid: &str, version: &str, deletable: bool, state: &str) -> Value {
        json!({
            "identifier": id, "runtimeIdentifier": rid, "version": version, "build": "X1",
            "platformIdentifier": "com.apple.platform.iphonesimulator",
            "deletable": deletable, "state": state, "sizeBytes": 7_000_000_000u64,
            "path": format!("/Library/Developer/CoreSimulator/Images/{id}.dmg")
        })
    }

    #[test]
    fn runtimes_with_devices_or_newest_are_kept() {
        let runtimes = json!({
            A: runtime(A, "com.apple.CoreSimulator.SimRuntime.iOS-17-4", "17.4", true, "Ready"),
            B: runtime(B, "com.apple.CoreSimulator.SimRuntime.iOS-18-6", "18.6", true, "Ready"),
            C: runtime(C, "com.apple.CoreSimulator.SimRuntime.iOS-26-0", "26.0", true, "Ready"),
            D: runtime(D, "com.apple.CoreSimulator.SimRuntime.iOS-16-4", "16.4", false, "Ready"),
        });
        let devices = json!({ "devices": {
            "com.apple.CoreSimulator.SimRuntime.iOS-18-6": [{ "udid": A, "isAvailable": true }],
            "com.apple.CoreSimulator.SimRuntime.iOS-17-4": []
        }});
        let r = removable_runtimes(&runtimes, &devices);
        // 17.4: no devices, not newest, deletable -> removable.
        // 18.6 has a device, 26.0 is newest, 16.4 is not deletable.
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].identifier, A);
        assert_eq!(r[0].label, "iOS 17.4 (X1)");
        assert_eq!(r[0].size, 7_000_000_000);
    }

    #[test]
    fn runtime_being_deleted_or_unmatchable_is_kept() {
        let mut no_rid = runtime(B, "x", "17.0", true, "Ready");
        no_rid.as_object_mut().unwrap().remove("runtimeIdentifier");
        let runtimes = json!({
            A: runtime(A, "com.apple.CoreSimulator.SimRuntime.iOS-17-4", "17.4", true, "Deleting"),
            B: no_rid,
            C: runtime(C, "com.apple.CoreSimulator.SimRuntime.iOS-26-0", "26.0", true, "Ready"),
        });
        assert!(removable_runtimes(&runtimes, &json!({"devices": {}})).is_empty());
    }

    #[test]
    fn unavailable_devices_are_listed() {
        let devices = json!({ "devices": {
            "com.apple.CoreSimulator.SimRuntime.iOS-15-0": [
                { "udid": A, "isAvailable": false },
                { "udid": "not-a-uuid", "isAvailable": false }
            ],
            "com.apple.CoreSimulator.SimRuntime.iOS-26-0": [{ "udid": B, "isAvailable": true }]
        }});
        assert_eq!(unavailable_devices(&devices), vec![A.to_string()]);
    }

    #[test]
    fn only_known_commands_are_allowed() {
        let ok =
            |a: &[&str]| is_allowed_command(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(ok(&["simctl", "delete", "unavailable"]));
        assert!(ok(&["simctl", "runtime", "delete", A]));
        assert!(!ok(&["simctl", "delete", "all"]));
        assert!(!ok(&["simctl", "erase", "all"]));
        assert!(!ok(&["simctl", "runtime", "delete", "all"]));
        assert!(!ok(&["simctl", "runtime", "delete", "17.4"]));
        assert!(!ok(&["simctl", "runtime", "delete", A, "--extra"]));
        assert!(!ok(&["rm", "-rf", "/"]));
    }
}
