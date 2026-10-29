mod access;
mod cleanup_engine;
mod history;

use cleanup_engine::{CleanupItem, DeleteMode, Env, Removed, ScannedItem};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Disks, System};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

/// Results of the last scan. Deletion only ever acts on items stored here.
#[derive(Default)]
struct ScanStore {
    items: HashMap<String, ScannedItem>,
    allowed_roots: Vec<PathBuf>,
    /// Bumped on every scan, so a failed cleanup never puts an item back into a newer scan.
    generation: u64,
}

/// Flag for the scan that is running now, so the UI can cancel it (PLAN 3.5).
#[derive(Default)]
struct ScanControl(Mutex<Arc<AtomicBool>>);

struct SysState {
    sys: Mutex<System>,
    disk_cache: Mutex<Option<(Instant, u64, u64)>>,
}

const DISK_REFRESH: Duration = Duration::from_secs(30);

fn lock_err<T>(_: T) -> String {
    "Internal state is unavailable. Restart Sweep.".to_string()
}

#[tauri::command]
async fn scan_environment(
    store: State<'_, Mutex<ScanStore>>,
    control: State<'_, ScanControl>,
    on_event: tauri::ipc::Channel<cleanup_engine::ScanProgress>,
    path: String,
    modules: Vec<String>,
    ignored_paths: Vec<String>,
    max_depth: Option<usize>,
) -> Result<Vec<CleanupItem>, String> {
    let base = PathBuf::from(&path);
    if path.is_empty() || !base.is_dir() {
        return Err("Choose an existing folder to scan.".to_string());
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut guard = control.0.lock().map_err(lock_err)?;
        *guard = cancelled.clone();
    }

    let result = tauri::async_runtime::spawn_blocking(move || {
        let env = Env::from_system();
        let running = cleanup_engine::running_process_names();
        let flag = &cancelled;
        let progress = |event| {
            let _ = on_event.send(event);
        };
        let hooks = cleanup_engine::ScanHooks {
            cancelled: &|| flag.load(Ordering::Relaxed),
            progress: &progress,
        };
        cleanup_engine::scan_directory_with_options(
            &base,
            &modules,
            &ignored_paths,
            &env,
            &running,
            &hooks,
            cleanup_engine::ScanOptions { max_depth },
        )
    })
    .await
    .map_err(|e| format!("Scan failed: {e}"))?;

    let Some(result) = result else {
        return Err("Scan cancelled.".to_string());
    };

    let items: Vec<CleanupItem> = result.items.iter().map(|s| s.item.clone()).collect();
    let mut guard = store.lock().map_err(lock_err)?;
    guard.items = result
        .items
        .into_iter()
        .map(|s| (s.item.id.clone(), s))
        .collect();
    guard.allowed_roots = result.allowed_roots;
    guard.generation += 1;
    Ok(items)
}

/// Cancel the scan that is running now. Does not touch any files.
#[tauri::command]
fn cancel_scan(control: State<'_, ScanControl>) {
    if let Ok(flag) = control.0.lock() {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Why a removal failed, and how many bytes it freed before failing (PLAN 3.7c).
#[derive(serde::Serialize, Debug)]
struct RemovalFailure {
    message: String,
    freed: u64,
}

impl From<String> for RemovalFailure {
    fn from(message: String) -> Self {
        Self { message, freed: 0 }
    }
}

impl From<&str> for RemovalFailure {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
            freed: 0,
        }
    }
}

/// Delete one item from the last scan. Returns the bytes freed and whether the
/// item went to the Trash.
#[tauri::command]
async fn cleanup_item(
    app: tauri::AppHandle,
    store: State<'_, Mutex<ScanStore>>,
    id: String,
    mode: Option<String>,
) -> Result<Removed, RemovalFailure> {
    let mode = match mode {
        None => DeleteMode::Permanent,
        Some(value) => DeleteMode::parse(&value).ok_or("Unknown delete mode.")?,
    };

    // Claim the item: a second request for the same id (a double-click, or React running an
    // effect twice in development) finds nothing and never deletes the same path concurrently.
    let (item, roots, generation) = {
        let mut guard = store.lock().map_err(lock_err)?;
        let item = guard
            .items
            .remove(&id)
            .ok_or("This item is not part of the last scan, or is already being removed.")?;
        (item, guard.allowed_roots.clone(), guard.generation)
    };

    let item_for_task = item.clone();
    let result: Result<Removed, RemovalFailure> = tauri::async_runtime::spawn_blocking(move || {
        let running = cleanup_engine::running_process_names();
        if let Some(app) = cleanup_engine::blocking_app(&item_for_task, &running) {
            return Err(format!("Close {app} first, then try again.").into());
        }
        let home = dirs::home_dir().ok_or("Cannot find your home folder.")?;
        let protected = cleanup_engine::protected_paths(&home);
        cleanup_engine::execute(&item_for_task, &roots, &protected, mode).map_err(|message| {
            RemovalFailure {
                message,
                freed: cleanup_engine::freed_after_failure(&item_for_task),
            }
        })
    })
    .await
    .map_err(|e| RemovalFailure::from(format!("Cleanup failed: {e}")))
    .and_then(|r| r);

    // Remember what this removal freed. History is a side effect: it never changes the result.
    let outcome = match &result {
        Ok(removed) => Ok((removed.bytes, removed.trashed)),
        Err(failure) => Err(failure.freed),
    };
    if let Some(entry) = history::entry_for(&item.item, outcome, history::now_secs()) {
        match app.path().app_data_dir() {
            Ok(dir) => history::record(&dir, &entry),
            Err(e) => eprintln!("Sweep: no data folder for the cleanup history: {e}"),
        }
    }

    match result {
        Ok(removed) => Ok(removed),
        Err(err) => {
            // Give the item back so the user can retry, unless a newer scan replaced the store.
            let mut guard = store.lock().map_err(lock_err)?;
            if guard.generation == generation {
                guard.items.insert(id, item);
            }
            Err(err)
        }
    }
}

/// Totals of everything Sweep has removed (PLAN 4.5).
#[tauri::command]
fn get_history(app: tauri::AppHandle) -> Result<history::HistorySummary, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(history::summarize(&history::load(&dir), history::now_secs()))
}

#[tauri::command]
fn clear_history(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    history::clear(&dir).map_err(|e| format!("Could not clear the history: {e}"))
}

#[tauri::command]
fn get_app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
async fn select_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("Could not open the folder picker: {e}"))
}

/// Show a scanned item in Finder (or the platform file manager).
#[tauri::command]
fn reveal_item(store: State<'_, Mutex<ScanStore>>, id: String) -> Result<(), String> {
    let path = {
        let guard = store.lock().map_err(lock_err)?;
        guard
            .items
            .get(&id)
            .map(|s| s.item.path.clone())
            .ok_or("This item is not part of the last scan. Scan again.")?
    };
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg("-R");
        c
    } else if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("explorer");
        c.arg("/select,");
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    let target = if cfg!(any(target_os = "macos", target_os = "windows")) {
        PathBuf::from(&path)
    } else {
        Path::new(&path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    };
    cmd.arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not show the item: {e}"))
}

/// Only web and email links may leave the app; never file:// or app URL schemes.
fn is_allowed_external(url: &str) -> bool {
    let scheme_ok = url.starts_with("https://") || url.starts_with("mailto:");
    scheme_ok
        && url.len() > "mailto:".len()
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Open an https or mailto link with the default app. Replaces the shell plugin (PLAN 0.6).
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !is_allowed_external(&url) {
        return Err("Only https and email links can be opened.".to_string());
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the link: {e}"))
}

/// Report whether macOS is hiding protected folders (Full Disk Access, PLAN 3.8).
/// On other systems this permission does not exist, so the answer is unknown.
#[tauri::command]
fn check_full_disk_access() -> access::FdaStatus {
    if cfg!(target_os = "macos") {
        match dirs::home_dir() {
            Some(home) => access::full_disk_access_at(&home),
            None => access::FdaStatus::Unknown,
        }
    } else {
        access::FdaStatus::Unknown
    }
}

/// Open the Full Disk Access pane in System Settings. Fixed URL, takes no arguments.
#[tauri::command]
fn open_privacy_settings() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg(access::PRIVACY_URL)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open Privacy settings: {e}"))
    } else {
        Err("Full Disk Access is only used on macOS.".to_string())
    }
}

#[derive(serde::Serialize)]
struct SystemInfo {
    os_name: String,
    os_version: String,
    cpu_usage: f32,
    ram_total: u64,
    ram_used: u64,
    disk_total: u64,
    disk_free: u64,
}

/// Space of the volume holding the home folder, counted once (PLAN 1.2).
fn home_disk_space() -> (u64, u64) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| home.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .or_else(|| disks.iter().find(|d| d.mount_point() == Path::new("/")))
        .map(|d| (d.total_space(), d.available_space()))
        .unwrap_or((0, 0))
}

#[tauri::command]
fn get_system_info(state: State<'_, SysState>) -> Result<SystemInfo, String> {
    let (cpu_usage, ram_total, ram_used) = {
        let mut sys = state.sys.lock().map_err(lock_err)?;
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        (
            sys.global_cpu_info().cpu_usage(),
            sys.total_memory(),
            sys.used_memory(),
        )
    };

    let (disk_total, disk_free) = {
        let mut cache = state.disk_cache.lock().map_err(lock_err)?;
        match *cache {
            Some((at, total, free)) if at.elapsed() < DISK_REFRESH => (total, free),
            _ => {
                let (total, free) = home_disk_space();
                *cache = Some((Instant::now(), total, free));
                (total, free)
            }
        }
    };

    Ok(SystemInfo {
        os_name: System::name().unwrap_or_else(|| "Unknown".to_string()),
        os_version: System::os_version().unwrap_or_else(|| "Unknown".to_string()),
        cpu_usage,
        ram_total,
        ram_used,
        disk_total,
        disk_free,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(ScanStore::default()))
        .manage(ScanControl::default())
        .manage(SysState {
            sys: Mutex::new(sys),
            disk_cache: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            scan_environment,
            cancel_scan,
            cleanup_item,
            get_history,
            clear_history,
            select_directory,
            open_external,
            reveal_item,
            get_system_info,
            get_app_version,
            check_full_disk_access,
            open_privacy_settings
        ])
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(|_, _| {}),
        Err(e) => eprintln!("Sweep failed to start: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::is_allowed_external;

    #[test]
    fn external_links() {
        assert!(is_allowed_external("https://github.com/abdulrasol"));
        assert!(is_allowed_external("mailto:abdulrsol97@gmail.com"));
        assert!(!is_allowed_external("file:///etc/passwd"));
        assert!(!is_allowed_external("x-apple.systempreferences:com.apple"));
        assert!(!is_allowed_external("http://example.com"));
        assert!(!is_allowed_external("https://a.com/ -a Calculator"));
        assert!(!is_allowed_external("mailto:"));
    }
}
