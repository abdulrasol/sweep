mod cleanup_engine;

use cleanup_engine::{CleanupItem, Env, ScannedItem};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{Disks, System};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

/// Results of the last scan. Deletion only ever acts on items stored here.
#[derive(Default)]
struct ScanStore {
    items: HashMap<String, ScannedItem>,
    allowed_roots: Vec<PathBuf>,
}

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
    path: String,
    modules: Vec<String>,
    ignored_paths: Vec<String>,
) -> Result<Vec<CleanupItem>, String> {
    let base = PathBuf::from(&path);
    if path.is_empty() || !base.is_dir() {
        return Err("Choose an existing folder to scan.".to_string());
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let env = Env::from_system();
        let running = cleanup_engine::running_process_names();
        cleanup_engine::scan_directory(&base, &modules, &ignored_paths, &env, &running)
    })
    .await
    .map_err(|e| format!("Scan failed: {e}"))?;

    let items: Vec<CleanupItem> = result.items.iter().map(|s| s.item.clone()).collect();
    let mut guard = store.lock().map_err(lock_err)?;
    guard.items = result
        .items
        .into_iter()
        .map(|s| (s.item.id.clone(), s))
        .collect();
    guard.allowed_roots = result.allowed_roots;
    Ok(items)
}

/// Delete one item from the last scan. Returns the bytes freed.
#[tauri::command]
async fn cleanup_item(store: State<'_, Mutex<ScanStore>>, id: String) -> Result<u64, String> {
    let (item, roots) = {
        let guard = store.lock().map_err(lock_err)?;
        let item = guard
            .items
            .get(&id)
            .cloned()
            .ok_or("This item is not part of the last scan. Scan again.")?;
        (item, guard.allowed_roots.clone())
    };

    let freed = item.item.size_bytes;
    let item_for_task = item.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let running = cleanup_engine::running_process_names();
        if let Some(app) = cleanup_engine::blocking_app(&item_for_task, &running) {
            return Err(format!("Close {app} first, then try again."));
        }
        let home = dirs::home_dir().ok_or("Cannot find your home folder.")?;
        let protected = cleanup_engine::protected_paths(&home);
        cleanup_engine::delete_path(&item_for_task, &roots, &protected)
    })
    .await
    .map_err(|e| format!("Cleanup failed: {e}"))??;

    store.lock().map_err(lock_err)?.items.remove(&id);
    Ok(freed)
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

/// Open an https link in the default browser. Replaces the shell plugin (PLAN 0.6).
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !url.starts_with("https://") || url.chars().any(char::is_whitespace) {
        return Err("Only https links can be opened.".to_string());
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(ScanStore::default()))
        .manage(SysState {
            sys: Mutex::new(sys),
            disk_cache: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            scan_environment,
            cleanup_item,
            select_directory,
            open_external,
            get_system_info,
            get_app_version
        ])
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(|_, _| {}),
        Err(e) => eprintln!("Sweep failed to start: {e}"),
    }
}
