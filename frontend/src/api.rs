//! Tauri bridge: typed async wrappers for every backend command.
//!
//! When `window.__TAURI__` is not present (plain browser via `trunk serve`)
//! every call is answered by `crate::mock` so the UI can be previewed.

use iclear_protocol::*;
use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen::prelude::*;

use crate::{mock, mock_cloud};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    async fn tauri_listen(
        event: &str,
        handler: &Closure<dyn FnMut(JsValue)>,
    ) -> Result<JsValue, JsValue>;
}

thread_local! {
    static IS_TAURI: bool = {
        js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("__TAURI__"))
            .map(|v| !v.is_undefined() && !v.is_null())
            .unwrap_or(false)
    };
}

/// `true` inside the Tauri webview, `false` in a plain browser (mock mode).
pub fn is_tauri() -> bool {
    IS_TAURI.with(|t| *t)
}

fn js_err(e: JsValue) -> String {
    if let Some(s) = e.as_string() {
        return s;
    }
    js_sys::JSON::stringify(&e)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_else(|| "Error desconocido".into())
}

fn to_js<A: Serialize>(args: &A) -> JsValue {
    args.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .unwrap_or(JsValue::UNDEFINED)
}

fn no_args() -> JsValue {
    js_sys::Object::new().into()
}

async fn call<T: DeserializeOwned>(cmd: &str, args: JsValue) -> Result<T, String> {
    let value = tauri_invoke(cmd, args).await.map_err(js_err)?;
    serde_wasm_bindgen::from_value(value).map_err(|e| format!("Respuesta inválida de {cmd}: {e}"))
}

// ---------------------------------------------------------------------------
// Argument payloads (Tauri expects camelCase keys)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KillArgs {
    pid: u32,
    force: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CategoryIdsArgs<'a> {
    category_ids: &'a [String],
}

#[derive(Serialize)]
struct PathsArgs<'a> {
    paths: &'a [String],
}

#[derive(Serialize)]
struct ExecuteArgs<'a> {
    token: &'a str,
    paths: &'a [String],
}

#[derive(Serialize)]
struct OptPathArgs<'a> {
    path: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LargeFilesArgs<'a> {
    path: Option<&'a str>,
    min_mb: u64,
    limit: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DuplicatesArgs<'a> {
    path: Option<&'a str>,
    min_mb: u64,
}

#[derive(Serialize)]
struct PathArgs<'a> {
    path: &'a str,
}

#[derive(Serialize)]
struct FanArgs {
    fan: u8,
}

#[derive(Serialize)]
struct FanRpmArgs {
    fan: u8,
    rpm: f32,
}

#[derive(Serialize)]
struct ChargeLimitArgs {
    percent: Option<u8>,
}

#[derive(Serialize)]
struct EnabledArgs {
    enabled: bool,
}

#[derive(Serialize)]
struct OptimizeArgs {
    action: OptimizeAction,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuitArgs {
    main_pid: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppPathArgs<'a> {
    app_path: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BundleIdsArgs<'a> {
    bundle_ids: &'a [String],
}

#[derive(Serialize)]
struct AgentArgs<'a> {
    path: &'a str,
    enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderArgs<'a> {
    provider_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderArgs<'a> {
    provider_id: &'a str,
    path: Option<&'a str>,
}

#[derive(Serialize)]
struct IdArgs {
    id: u64,
}

#[derive(Serialize)]
struct StrIdArgs<'a> {
    id: &'a str,
}

#[derive(Serialize)]
struct DraftArgs<'a> {
    draft: &'a BackupDraft,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RestoreArgs<'a> {
    id: &'a str,
    version_path: &'a str,
    target: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub async fn get_machine_info() -> Result<MachineInfo, String> {
    if !is_tauri() {
        return Ok(mock::machine_info());
    }
    call("get_machine_info", no_args()).await
}

pub async fn get_metrics() -> Result<Metrics, String> {
    if !is_tauri() {
        return Ok(mock::metrics());
    }
    call("get_metrics", no_args()).await
}

pub async fn get_processes() -> Result<Vec<ProcessInfo>, String> {
    if !is_tauri() {
        return Ok(mock::processes());
    }
    call("get_processes", no_args()).await
}

pub async fn kill_process(pid: u32, force: bool) -> Result<(), String> {
    if !is_tauri() {
        return mock::kill_process(pid, force).await;
    }
    call("kill_process", to_js(&KillArgs { pid, force })).await
}

pub async fn cleanup_categories() -> Result<Vec<CleanupCategory>, String> {
    if !is_tauri() {
        return Ok(mock::cleanup_categories().await);
    }
    call("cleanup_categories", no_args()).await
}

pub async fn preview_cleanup(category_ids: Vec<String>) -> Result<CleanupPreview, String> {
    if !is_tauri() {
        return Ok(mock::preview_cleanup(&category_ids).await);
    }
    call("preview_cleanup", to_js(&CategoryIdsArgs { category_ids: &category_ids })).await
}

pub async fn preview_paths(paths: Vec<String>) -> Result<CleanupPreview, String> {
    if !is_tauri() {
        return Ok(mock::preview_paths(&paths).await);
    }
    call("preview_paths", to_js(&PathsArgs { paths: &paths })).await
}

pub async fn execute_cleanup(token: String, paths: Vec<String>) -> Result<DeleteReport, String> {
    if !is_tauri() {
        return mock::execute_cleanup(&token, &paths).await;
    }
    call("execute_cleanup", to_js(&ExecuteArgs { token: &token, paths: &paths })).await
}

pub async fn disk_usage(path: Option<String>) -> Result<UsageReport, String> {
    if !is_tauri() {
        return Ok(mock::disk_usage(path.as_deref()).await);
    }
    call("disk_usage", to_js(&OptPathArgs { path: path.as_deref() })).await
}

pub async fn large_files(
    path: Option<String>,
    min_mb: u64,
    limit: usize,
) -> Result<Vec<LargeFile>, String> {
    if !is_tauri() {
        return Ok(mock::large_files(min_mb, limit).await);
    }
    call(
        "large_files",
        to_js(&LargeFilesArgs { path: path.as_deref(), min_mb, limit }),
    )
    .await
}

pub async fn find_duplicates(path: Option<String>, min_mb: u64) -> Result<Vec<DuplicateGroup>, String> {
    if !is_tauri() {
        return Ok(mock::find_duplicates(min_mb).await);
    }
    call("find_duplicates", to_js(&DuplicatesArgs { path: path.as_deref(), min_mb })).await
}

pub async fn reveal_in_finder(path: String) -> Result<(), String> {
    if !is_tauri() {
        return Ok(());
    }
    call("reveal_in_finder", to_js(&PathArgs { path: &path })).await
}

pub async fn helper_status() -> Result<HelperStatus, String> {
    if !is_tauri() {
        return Ok(mock::helper_status());
    }
    call("helper_status", no_args()).await
}

pub async fn install_helper() -> Result<HelperStatus, String> {
    if !is_tauri() {
        return Ok(mock::install_helper().await);
    }
    call("install_helper", no_args()).await
}

pub async fn uninstall_helper() -> Result<HelperStatus, String> {
    if !is_tauri() {
        return Ok(mock::uninstall_helper().await);
    }
    call("uninstall_helper", no_args()).await
}

pub async fn set_fan_auto(fan: u8) -> Result<HelperState, String> {
    if !is_tauri() {
        return mock::set_fan_auto(fan).await;
    }
    call("set_fan_auto", to_js(&FanArgs { fan })).await
}

pub async fn set_fan_rpm(fan: u8, rpm: f32) -> Result<HelperState, String> {
    if !is_tauri() {
        return mock::set_fan_rpm(fan, rpm).await;
    }
    call("set_fan_rpm", to_js(&FanRpmArgs { fan, rpm })).await
}

pub async fn set_charge_limit(percent: Option<u8>) -> Result<HelperState, String> {
    if !is_tauri() {
        return mock::set_charge_limit(percent).await;
    }
    call("set_charge_limit", to_js(&ChargeLimitArgs { percent })).await
}

pub async fn set_force_discharge(enabled: bool) -> Result<HelperState, String> {
    if !is_tauri() {
        return mock::set_force_discharge(enabled).await;
    }
    call("set_force_discharge", to_js(&EnabledArgs { enabled })).await
}

pub async fn run_optimization(action: OptimizeAction) -> Result<String, String> {
    if !is_tauri() {
        return mock::run_optimization(action).await;
    }
    call("run_optimization", to_js(&OptimizeArgs { action })).await
}

pub async fn list_launch_agents() -> Result<Vec<LaunchAgent>, String> {
    if !is_tauri() {
        return Ok(mock::launch_agents().await);
    }
    call("list_launch_agents", no_args()).await
}

pub async fn set_launch_agent_enabled(path: String, enabled: bool) -> Result<(), String> {
    if !is_tauri() {
        return mock::set_launch_agent_enabled(&path, enabled).await;
    }
    call("set_launch_agent_enabled", to_js(&AgentArgs { path: &path, enabled })).await
}

pub async fn get_memory_report() -> Result<MemoryReport, String> {
    if !is_tauri() {
        return Ok(mock::memory_report());
    }
    call("get_memory_report", no_args()).await
}

/// Gracefully quits a GUI app (AppleScript quit) or sends SIGTERM.
pub async fn quit_app(main_pid: u32) -> Result<(), String> {
    if !is_tauri() {
        return mock::quit_app(main_pid).await;
    }
    call("quit_app", to_js(&QuitArgs { main_pid })).await
}

/// Slow: sizes every app bundle and its leftovers.
pub async fn get_apps_report() -> Result<AppsReport, String> {
    if !is_tauri() {
        return Ok(mock::apps_report().await);
    }
    call("get_apps_report", no_args()).await
}

pub async fn preview_uninstall(app_path: String) -> Result<CleanupPreview, String> {
    if !is_tauri() {
        return mock::preview_uninstall(&app_path).await;
    }
    call("preview_uninstall", to_js(&AppPathArgs { app_path: &app_path })).await
}

pub async fn preview_orphans(bundle_ids: Vec<String>) -> Result<CleanupPreview, String> {
    if !is_tauri() {
        return Ok(mock::preview_orphans(&bundle_ids).await);
    }
    call("preview_orphans", to_js(&BundleIdsArgs { bundle_ids: &bundle_ids })).await
}

pub async fn open_full_disk_access_settings() -> Result<(), String> {
    if !is_tauri() {
        return Ok(());
    }
    call("open_full_disk_access_settings", no_args()).await
}

pub async fn open_battery_settings() -> Result<(), String> {
    if !is_tauri() {
        return Ok(());
    }
    call("open_battery_settings", no_args()).await
}

// ---- cloud ----------------------------------------------------------------

pub async fn list_cloud_providers() -> Result<Vec<CloudProvider>, String> {
    if !is_tauri() {
        return Ok(mock_cloud::providers());
    }
    call("list_cloud_providers", no_args()).await
}

/// Figures of one drive; the first call starts a background scan and the
/// numbers grow while `scanning` is true.
pub async fn cloud_stats(provider_id: String) -> Result<CloudStats, String> {
    if !is_tauri() {
        return mock_cloud::stats(&provider_id, false).await;
    }
    call("cloud_stats", to_js(&ProviderArgs { provider_id: &provider_id })).await
}

pub async fn rescan_cloud(provider_id: String) -> Result<CloudStats, String> {
    if !is_tauri() {
        return mock_cloud::stats(&provider_id, true).await;
    }
    call("rescan_cloud", to_js(&ProviderArgs { provider_id: &provider_id })).await
}

pub async fn list_cloud_folder(provider_id: String, path: Option<String>) -> Result<CloudListing, String> {
    if !is_tauri() {
        return mock_cloud::list(&provider_id, path.as_deref()).await;
    }
    call("list_cloud_folder", to_js(&FolderArgs { provider_id: &provider_id, path: path.as_deref() })).await
}

pub async fn cloud_download(path: String) -> Result<Transfer, String> {
    if !is_tauri() {
        return mock_cloud::download(&path);
    }
    call("cloud_download", to_js(&PathArgs { path: &path })).await
}

pub async fn cloud_free_up(path: String) -> Result<CloudActionReport, String> {
    if !is_tauri() {
        return mock_cloud::free_up(&path).await;
    }
    call("cloud_free_up", to_js(&PathArgs { path: &path })).await
}

pub async fn list_transfers() -> Result<Vec<Transfer>, String> {
    if !is_tauri() {
        return Ok(mock_cloud::transfers());
    }
    call("list_transfers", no_args()).await
}

pub async fn cancel_transfer(id: u64) -> Result<(), String> {
    if !is_tauri() {
        return mock_cloud::cancel(id);
    }
    call("cancel_transfer", to_js(&IdArgs { id })).await
}

pub async fn clear_transfers() -> Result<(), String> {
    if !is_tauri() {
        mock_cloud::clear_transfers();
        return Ok(());
    }
    call("clear_transfers", no_args()).await
}

pub async fn list_backups() -> Result<Vec<BackupJob>, String> {
    if !is_tauri() {
        return Ok(mock_cloud::backups());
    }
    call("list_backups", no_args()).await
}

pub async fn save_backup(draft: BackupDraft) -> Result<BackupJob, String> {
    if !is_tauri() {
        return mock_cloud::save_backup(draft);
    }
    call("save_backup", to_js(&DraftArgs { draft: &draft })).await
}

pub async fn delete_backup(id: String) -> Result<(), String> {
    if !is_tauri() {
        return mock_cloud::delete_backup(&id);
    }
    call("delete_backup", to_js(&StrIdArgs { id: &id })).await
}

pub async fn run_backup(id: String) -> Result<Transfer, String> {
    if !is_tauri() {
        return mock_cloud::run_backup(&id);
    }
    call("run_backup", to_js(&StrIdArgs { id: &id })).await
}

pub async fn backup_versions(id: String) -> Result<Vec<BackupVersion>, String> {
    if !is_tauri() {
        return mock_cloud::versions(&id).await;
    }
    call("backup_versions", to_js(&StrIdArgs { id: &id })).await
}

pub async fn restore_backup(id: String, version_path: String, target: Option<String>) -> Result<Transfer, String> {
    if !is_tauri() {
        return mock_cloud::restore(&id, &version_path);
    }
    call(
        "restore_backup",
        to_js(&RestoreArgs { id: &id, version_path: &version_path, target: target.as_deref() }),
    )
    .await
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Subscribes to the backend `metrics` event (every ~1.5 s). In mock mode a
/// timer produces fake samples at the same cadence.
pub fn on_metrics(cb: impl Fn(Metrics) + 'static) {
    if is_tauri() {
        let handler = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
            let payload = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
                .unwrap_or(JsValue::UNDEFINED);
            match serde_wasm_bindgen::from_value::<Metrics>(payload) {
                Ok(m) => cb(m),
                Err(e) => web_sys::console::warn_1(&format!("metrics inválidas: {e}").into()),
            }
        });
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = tauri_listen("metrics", &handler).await {
                web_sys::console::error_1(&e);
            }
            // The subscription lives for the whole app lifetime.
            handler.forget();
        });
    } else {
        let tick = Closure::<dyn FnMut()>::new(move || cb(mock::metrics()));
        if let Some(w) = web_sys::window() {
            let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                1500,
            );
        }
        tick.forget();
    }
}

/// Promise-based sleep, used by mocks to simulate slow scans.
pub async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
