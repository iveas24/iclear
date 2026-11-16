//! iclear desktop app: exposes `iclear-core` to the Leptos UI as Tauri
//! commands and pushes a `metrics` event every ~1.5 s. The command contract
//! is documented in docs/ARCHITECTURE.md.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iclear_core::storage::{self, categories, PreviewStore};
use iclear_core::system::Collector;
use iclear_core::memory::MemoryIntel;
use iclear_core::cloud::{self, BackupService, Scans, Transfers};
use iclear_core::{apps, helper_client, launchd, memory, system};
use iclear_protocol::*;
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State};

const METRICS_INTERVAL: Duration = Duration::from_millis(1500);

struct AppState {
    collector: Mutex<Collector>,
    latest: Arc<Mutex<Option<Metrics>>>,
    previews: Arc<PreviewStore>,
    intel: Arc<Mutex<MemoryIntel>>,
    scans: Arc<Scans>,
    transfers: Arc<Transfers>,
    backups: Arc<BackupService>,
}

type CmdResult<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

fn root_or_home(path: Option<String>) -> PathBuf {
    path.filter(|p| !p.is_empty()).map(PathBuf::from).unwrap_or_else(storage::home)
}

/// Folders skipped by large-file and duplicate scans (system-managed data).
fn scan_skips() -> Vec<PathBuf> {
    let h = storage::home();
    ["Library", ".Trash", "Pictures/Photos Library.photoslibrary"].iter().map(|p| h.join(p)).collect()
}

// ---- system ---------------------------------------------------------------

#[tauri::command]
async fn get_machine_info() -> CmdResult<MachineInfo> {
    blocking(system::machine_info).await
}

#[tauri::command]
fn get_metrics(state: State<'_, AppState>) -> CmdResult<Metrics> {
    if let Some(m) = state.latest.lock().unwrap().clone() {
        return Ok(m);
    }
    Ok(state.collector.lock().unwrap().metrics())
}

#[tauri::command]
fn get_processes(state: State<'_, AppState>) -> CmdResult<Vec<ProcessInfo>> {
    Ok(state.collector.lock().unwrap().processes())
}

#[tauri::command]
fn kill_process(state: State<'_, AppState>, pid: u32, force: bool) -> CmdResult<()> {
    state.collector.lock().unwrap().kill(pid, force)
}

// ---- memory & apps --------------------------------------------------------

#[tauri::command]
async fn get_memory_report(state: State<'_, AppState>) -> CmdResult<MemoryReport> {
    let intel = state.intel.clone();
    blocking(move || intel.lock().unwrap().report()).await
}

#[tauri::command]
async fn quit_app(main_pid: u32) -> CmdResult<()> {
    blocking(move || memory::quit_app(main_pid)).await?
}

#[tauri::command]
async fn get_apps_report(state: State<'_, AppState>) -> CmdResult<AppsReport> {
    let focus = state.intel.lock().unwrap().focus_map().clone();
    blocking(move || apps::scan(&storage::home(), &focus)).await
}

#[tauri::command]
async fn preview_uninstall(state: State<'_, AppState>, app_path: String) -> CmdResult<CleanupPreview> {
    let store = state.previews.clone();
    blocking(move || apps::uninstall_items(&storage::home(), &PathBuf::from(app_path)).map(|items| store.create_app_removal(items))).await?
}

#[tauri::command]
async fn preview_orphans(state: State<'_, AppState>, bundle_ids: Vec<String>) -> CmdResult<CleanupPreview> {
    let store = state.previews.clone();
    blocking(move || store.create_app_removal(apps::orphan_items(&storage::home(), &bundle_ids))).await
}

// ---- storage --------------------------------------------------------------

#[tauri::command]
async fn cleanup_categories() -> CmdResult<Vec<CleanupCategory>> {
    blocking(|| storage::summaries(&storage::home())).await
}

#[tauri::command]
async fn preview_cleanup(state: State<'_, AppState>, category_ids: Vec<String>) -> CmdResult<CleanupPreview> {
    let store = state.previews.clone();
    blocking(move || {
        let home = storage::home();
        let items = category_ids
            .iter()
            .filter_map(|id| categories::find(id))
            .flat_map(|c| c.items(&home))
            .collect();
        store.create(items)
    })
    .await
}

#[tauri::command]
async fn preview_paths(state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<CleanupPreview> {
    let store = state.previews.clone();
    blocking(move || store.create_for_paths(&paths)).await
}

#[tauri::command]
async fn execute_cleanup(state: State<'_, AppState>, token: String, paths: Vec<String>) -> CmdResult<DeleteReport> {
    let store = state.previews.clone();
    blocking(move || store.execute(&token, &paths)).await?
}

#[tauri::command]
async fn disk_usage(path: Option<String>) -> CmdResult<UsageReport> {
    let root = root_or_home(path);
    blocking(move || storage::scan::usage(&root).map_err(|e| e.to_string())).await?
}

#[tauri::command]
async fn large_files(path: Option<String>, min_mb: u64, limit: usize) -> CmdResult<Vec<LargeFile>> {
    let root = root_or_home(path);
    blocking(move || storage::scan::large_files(&root, min_mb.max(1) * 1_000_000, limit.clamp(1, 500), &scan_skips()))
        .await
}

#[tauri::command]
async fn find_duplicates(path: Option<String>, min_mb: u64) -> CmdResult<Vec<DuplicateGroup>> {
    let root = root_or_home(path);
    blocking(move || storage::duplicates::find(&root, min_mb.max(1) * 1_000_000, &scan_skips())).await
}

#[tauri::command]
fn reveal_in_finder(path: String) -> CmdResult<()> {
    Command::new("/usr/bin/open").args(["-R", &path]).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_full_disk_access_settings() -> CmdResult<()> {
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
}

#[tauri::command]
fn open_battery_settings() -> CmdResult<()> {
    open_url("x-apple.systempreferences:com.apple.Battery-Settings.extension")
}

fn open_url(url: &str) -> CmdResult<()> {
    Command::new("/usr/bin/open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// ---- cloud ----------------------------------------------------------------

#[tauri::command]
async fn list_cloud_providers() -> CmdResult<Vec<CloudProvider>> {
    blocking(|| cloud::providers(&storage::home())).await
}

#[tauri::command]
async fn cloud_stats(state: State<'_, AppState>, provider_id: String) -> CmdResult<CloudStats> {
    let scans = state.scans.clone();
    blocking(move || scans.stats(&storage::home(), &provider_id)).await?
}

#[tauri::command]
async fn rescan_cloud(state: State<'_, AppState>, provider_id: String) -> CmdResult<CloudStats> {
    let scans = state.scans.clone();
    blocking(move || {
        let home = storage::home();
        scans.start(&home, &provider_id, true)?;
        scans.stats(&home, &provider_id)
    })
    .await?
}

#[tauri::command]
async fn list_cloud_folder(state: State<'_, AppState>, provider_id: String, path: Option<String>) -> CmdResult<CloudListing> {
    let scans = state.scans.clone();
    blocking(move || cloud::list(&storage::home(), &scans, &provider_id, path.as_deref())).await?
}

#[tauri::command]
async fn cloud_download(state: State<'_, AppState>, path: String) -> CmdResult<Transfer> {
    let transfers = state.transfers.clone();
    blocking(move || cloud::transfers::download(&transfers, &storage::home(), &path)).await?
}

#[tauri::command]
async fn cloud_free_up(path: String) -> CmdResult<CloudActionReport> {
    blocking(move || cloud::evict(&storage::home(), &path)).await?
}

#[tauri::command]
fn list_transfers(state: State<'_, AppState>) -> CmdResult<Vec<Transfer>> {
    Ok(state.transfers.list())
}

#[tauri::command]
fn cancel_transfer(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    state.transfers.cancel(id)
}

#[tauri::command]
fn clear_transfers(state: State<'_, AppState>) -> CmdResult<()> {
    state.transfers.clear_finished();
    Ok(())
}

#[tauri::command]
fn list_backups(state: State<'_, AppState>) -> CmdResult<Vec<BackupJob>> {
    Ok(state.backups.list())
}

#[tauri::command]
async fn save_backup(state: State<'_, AppState>, draft: BackupDraft) -> CmdResult<BackupJob> {
    let backups = state.backups.clone();
    blocking(move || backups.save_job(draft)).await?
}

#[tauri::command]
fn delete_backup(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.backups.delete_job(&id)
}

#[tauri::command]
fn run_backup(state: State<'_, AppState>, id: String) -> CmdResult<Transfer> {
    state.backups.run(&id)
}

#[tauri::command]
async fn backup_versions(state: State<'_, AppState>, id: String) -> CmdResult<Vec<BackupVersion>> {
    let backups = state.backups.clone();
    blocking(move || backups.versions(&id)).await?
}

#[tauri::command]
async fn restore_backup(state: State<'_, AppState>, id: String, version_path: String, target: Option<String>) -> CmdResult<Transfer> {
    let backups = state.backups.clone();
    blocking(move || backups.restore(&id, &version_path, target)).await?
}

// ---- helper ---------------------------------------------------------------

#[tauri::command]
async fn helper_status() -> CmdResult<HelperStatus> {
    blocking(helper_client::status).await
}

#[tauri::command]
async fn install_helper() -> CmdResult<HelperStatus> {
    blocking(|| {
        let bin = helper_client::bundled_helper_path().ok_or(
            "No se encontró iclear-helper junto a la app. En desarrollo ejecuta `cargo build -p iclear-helper`.".to_string(),
        )?;
        helper_client::install(&bin)?;
        // launchd needs a moment to start the daemon.
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(250));
            let s = helper_client::status();
            if s.reachable {
                return Ok(s);
            }
        }
        Ok(helper_client::status())
    })
    .await?
}

#[tauri::command]
async fn uninstall_helper() -> CmdResult<HelperStatus> {
    blocking(|| helper_client::uninstall().map(|_| helper_client::status())).await?
}

async fn helper_call(req: HelperRequest) -> CmdResult<HelperResponse> {
    blocking(move || helper_client::call(&req)).await?
}

async fn helper_state(req: HelperRequest) -> CmdResult<HelperState> {
    helper_call(req).await?.state.ok_or_else(|| "El helper no devolvió su estado".to_string())
}

#[tauri::command]
async fn set_fan_auto(fan: u8) -> CmdResult<HelperState> {
    helper_state(HelperRequest::SetFanAuto { fan }).await
}

#[tauri::command]
async fn set_fan_rpm(fan: u8, rpm: f32) -> CmdResult<HelperState> {
    helper_state(HelperRequest::SetFanRpm { fan, rpm }).await
}

#[tauri::command]
async fn set_charge_limit(percent: Option<u8>) -> CmdResult<HelperState> {
    helper_state(HelperRequest::SetChargeLimit { percent }).await
}

#[tauri::command]
async fn set_force_discharge(enabled: bool) -> CmdResult<HelperState> {
    helper_state(HelperRequest::SetForceDischarge { enabled }).await
}

#[tauri::command]
async fn run_optimization(action: OptimizeAction) -> CmdResult<String> {
    Ok(helper_call(HelperRequest::Optimize { action }).await?.message)
}

// ---- launch agents --------------------------------------------------------

#[tauri::command]
async fn list_launch_agents() -> CmdResult<Vec<LaunchAgent>> {
    blocking(launchd::list).await
}

#[tauri::command]
async fn set_launch_agent_enabled(path: String, enabled: bool) -> CmdResult<()> {
    blocking(move || launchd::set_enabled(&path, enabled)).await?
}

// ---- background -----------------------------------------------------------

fn tray_title(m: &Metrics, transfers: &[Transfer]) -> String {
    let mut parts = Vec::new();
    // Active cloud transfer: "↑ 42%" / "↓ 42%".
    if let Some(t) = transfers.iter().find(|t| !t.phase.is_finished()) {
        let (arrow, pct) = match t.phase {
            TransferPhase::Uploading => ("↑", t.upload_pct),
            TransferPhase::Downloading => ("↓", t.download_pct),
            _ if t.kind == TransferKind::Download => ("↓", t.download_pct),
            _ => ("↑", t.copy_pct),
        };
        parts.push(format!("{arrow}{pct:.0}%"));
    }
    if let Some(b) = &m.battery {
        parts.push(format!("{}%{}", b.percent, if b.is_charging { "⚡" } else { "" }));
    }
    if let Some(t) = m.temp_summary.cpu_avg {
        parts.push(format!("{t:.0}°"));
    }
    parts.join(" · ")
}

fn main() {
    let latest = Arc::new(Mutex::new(None));
    let transfers = Transfers::new();
    let backups = BackupService::new(storage::home(), transfers.clone());
    let state = AppState {
        collector: Mutex::new(Collector::new()),
        latest: latest.clone(),
        previews: Arc::new(PreviewStore::new(storage::home())),
        intel: Arc::new(Mutex::new(MemoryIntel::new())),
        scans: Scans::new(),
        transfers: transfers.clone(),
        backups: backups.clone(),
    };
    let intel = state.intel.clone();

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            let tray = TrayIconBuilder::with_id("iclear")
                .icon(app.default_window_icon().cloned().expect("icon"))
                .tooltip("iclear")
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        if let Some(w) = tray.app_handle().get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut collector = Collector::new();
                let mut ticks: u32 = 0;
                loop {
                    std::thread::sleep(METRICS_INTERVAL);
                    // Every ~6 s: which app is in front, and periodic footprint samples.
                    ticks = ticks.wrapping_add(1);
                    if ticks % 4 == 0 {
                        if let Ok(mut i) = intel.try_lock() {
                            i.tick();
                        }
                    }
                    // Every ~60 s: start scheduled cloud backups that are due.
                    if ticks % 40 == 0 {
                        backups.tick();
                    }
                    let m = collector.metrics();
                    let _ = tray.set_title(Some(tray_title(&m, &transfers.list())));
                    let _ = handle.emit("metrics", &m);
                    *latest.lock().unwrap() = Some(m);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_machine_info,
            get_metrics,
            get_processes,
            kill_process,
            cleanup_categories,
            preview_cleanup,
            preview_paths,
            execute_cleanup,
            disk_usage,
            large_files,
            find_duplicates,
            reveal_in_finder,
            open_full_disk_access_settings,
            open_battery_settings,
            helper_status,
            install_helper,
            uninstall_helper,
            set_fan_auto,
            set_fan_rpm,
            set_charge_limit,
            set_force_discharge,
            run_optimization,
            list_launch_agents,
            set_launch_agent_enabled,
            get_memory_report,
            quit_app,
            get_apps_report,
            preview_uninstall,
            preview_orphans,
            list_cloud_providers,
            cloud_stats,
            rescan_cloud,
            list_cloud_folder,
            cloud_download,
            cloud_free_up,
            list_transfers,
            cancel_transfer,
            clear_transfers,
            list_backups,
            save_backup,
            delete_backup,
            run_backup,
            backup_versions,
            restore_backup,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar iclear");
}
