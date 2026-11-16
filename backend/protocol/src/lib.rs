//! Shared data types used by the Tauri app, the privileged helper and the
//! Leptos frontend. Everything here is plain serde data: no OS calls.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Live metrics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SystemSnapshot {
    pub cpu_total: f32,
    pub cpu_per_core: Vec<f32>,
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub load_avg: [f64; 3],
    pub uptime_secs: u64,
    pub net_rx_per_sec: u64,
    pub net_tx_per_sec: u64,
    pub disks: Vec<DiskInfo>,
    pub process_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BatteryInfo {
    pub percent: u8,
    pub is_charging: bool,
    pub external_connected: bool,
    pub fully_charged: bool,
    pub cycle_count: u32,
    pub design_capacity_mah: u32,
    pub max_capacity_mah: u32,
    pub current_capacity_mah: u32,
    /// max_capacity / design_capacity, 0-100+.
    pub health_percent: f32,
    pub temperature_c: f32,
    pub voltage_v: f32,
    pub amperage_ma: i32,
    /// Positive while charging, negative while discharging.
    pub power_w: f32,
    pub time_remaining_min: Option<u32>,
    pub adapter_watts: Option<u32>,
    pub adapter_name: Option<String>,
    /// Reason code reported by the charger when not charging (0 = none).
    pub not_charging_reason: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FanMode {
    Auto,
    Manual,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanInfo {
    pub index: u8,
    pub actual_rpm: f32,
    pub min_rpm: f32,
    pub max_rpm: f32,
    pub target_rpm: Option<f32>,
    pub mode: FanMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TempSensor {
    pub key: String,
    pub label: String,
    pub group: String,
    pub celsius: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TempSummary {
    pub cpu_avg: Option<f32>,
    pub cpu_max: Option<f32>,
    pub gpu_avg: Option<f32>,
    pub battery: Option<f32>,
    pub ssd: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Metrics {
    pub timestamp_ms: u64,
    pub system: SystemSnapshot,
    pub battery: Option<BatteryInfo>,
    pub fans: Vec<FanInfo>,
    pub temps: Vec<TempSensor>,
    pub temp_summary: TempSummary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MachineInfo {
    pub model: String,
    pub chip: String,
    pub os_version: String,
    pub hostname: String,
    pub cpu_cores: usize,
    pub mem_total: u64,
}

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
    pub user: Option<String>,
    pub killable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LaunchAgent {
    pub label: String,
    pub path: String,
    /// "user" (~/Library/LaunchAgents), "global-agent" or "global-daemon".
    pub scope: String,
    pub loaded: bool,
    pub disabled: bool,
    pub program: Option<String>,
}

// ---------------------------------------------------------------------------
// Storage / cleanup
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Risk {
    /// Regenerated automatically (caches, logs).
    Safe,
    /// Can be rebuilt but costs time or bandwidth (Xcode, dev caches).
    Moderate,
    /// User data that may matter (downloads, backups).
    Review,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CleanupCategory {
    pub id: String,
    pub name: String,
    pub description: String,
    pub roots: Vec<String>,
    pub size_bytes: u64,
    pub item_count: usize,
    pub risk: Risk,
    pub requires_helper: bool,
    pub selected_by_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreviewItem {
    pub path: String,
    pub size_bytes: u64,
    pub is_dir: bool,
    pub category_id: String,
    pub modified_days_ago: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CleanupPreview {
    /// Must be sent back to confirm the deletion. Expires after a few minutes.
    pub token: String,
    pub items: Vec<PreviewItem>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DeleteReport {
    pub deleted: usize,
    pub freed_bytes: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirUsage {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageReport {
    pub root: String,
    pub total_bytes: u64,
    pub children: Vec<DirUsage>,
    pub skipped_errors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LargeFile {
    pub path: String,
    pub size_bytes: u64,
    pub modified_days_ago: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size_bytes: u64,
    pub paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// Memory intelligence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MemoryPressure {
    #[default]
    Normal,
    Warning,
    Critical,
}

/// Same buckets as Activity Monitor.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MemoryBreakdown {
    pub total: u64,
    /// Anonymous memory of apps (what they actually ask for).
    pub app: u64,
    /// Kernel and drivers; cannot be freed.
    pub wired: u64,
    /// RAM used by the compressor.
    pub compressed: u64,
    /// Raw size of what is stored compressed (before compression).
    pub compressed_original: u64,
    /// File cache; macOS frees it instantly when needed.
    pub cached: u64,
    pub free: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub pressure: MemoryPressure,
    /// kern.memorystatus_level (0-100).
    pub free_percent: u8,
    /// Pages swapped out since boot (bytes).
    pub swapouts_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AppKind {
    App,
    Browser,
    Electron,
    Developer,
    Virtualization,
    Background,
    System,
}

/// Processes grouped by owning app (helpers and renderers are summed).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppMemory {
    /// Bundle path, or process name for loose binaries.
    pub id: String,
    pub name: String,
    pub bundle_id: Option<String>,
    pub kind: AppKind,
    /// Sum of phys_footprint (what Activity Monitor calls "Memory").
    pub footprint_bytes: u64,
    pub process_count: usize,
    /// Renderer/tab-like helper processes (browsers, Electron).
    pub renderer_count: usize,
    pub cpu: f32,
    pub pids: Vec<u32>,
    /// Main process pid, used to quit the whole app.
    pub main_pid: Option<u32>,
    pub killable: bool,
    pub is_gui: bool,
    pub running_secs: u64,
    /// Seconds since the app was last in front (only while iclear observes).
    pub last_focused_secs_ago: Option<u64>,
    /// Footprint growth over the observed window, in bytes per hour.
    pub growth_per_hour: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Suggestion,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InsightAction {
    QuitApp { app_id: String, main_pid: u32 },
    Optimize { action: OptimizeAction },
    /// Navigate to a page of the UI ("apps", "optimize", "cleanup", ...).
    OpenPage { page: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Insight {
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub savings_bytes: Option<u64>,
    pub action: Option<InsightAction>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MemoryReport {
    pub breakdown: MemoryBreakdown,
    pub apps: Vec<AppMemory>,
    pub insights: Vec<Insight>,
    /// How long iclear has been observing (focus and growth need time).
    pub observed_secs: u64,
    /// 0-100: overall memory health (100 = no pressure).
    pub score: u8,
}

// ---------------------------------------------------------------------------
// Installed apps / unused detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppLeftover {
    pub path: String,
    /// "Soporte", "Caché", "Preferencias", "Contenedor", ...
    pub kind: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageVerdict {
    /// Used in the last 30 days.
    Active,
    /// 30-90 days.
    Occasional,
    /// More than 90 days, or never opened.
    Unused,
    /// No usage data (Spotlight disabled or app never indexed).
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledApp {
    pub path: String,
    pub name: String,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub size_bytes: u64,
    pub leftovers: Vec<AppLeftover>,
    pub leftovers_bytes: u64,
    pub last_used_days_ago: Option<u64>,
    pub use_count: Option<u64>,
    pub added_days_ago: Option<u64>,
    pub is_running: bool,
    pub is_electron: bool,
    pub from_app_store: bool,
    /// Has a LaunchAgent/LoginItem that starts it in the background.
    pub launches_at_login: bool,
    pub verdict: UsageVerdict,
    /// Human explanation of the verdict.
    pub reason: String,
    /// False for Apple apps, running apps or bundles we can't write.
    pub removable: bool,
}

/// Support files of apps that are no longer installed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrphanGroup {
    pub bundle_id: String,
    pub leftovers: Vec<AppLeftover>,
    pub total_bytes: u64,
    pub modified_days_ago: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AppsReport {
    pub apps: Vec<InstalledApp>,
    pub orphans: Vec<OrphanGroup>,
    /// Bundle + leftovers of apps with verdict Unused.
    pub unused_bytes: u64,
    pub orphans_bytes: u64,
}

// ---------------------------------------------------------------------------
// Privileged helper protocol (JSON, one message per line over a Unix socket)
// ---------------------------------------------------------------------------

pub const HELPER_SOCKET: &str = "/var/run/iclear-helper.sock";
pub const HELPER_LABEL: &str = "com.iclear.helper";
pub const HELPER_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OptimizeAction {
    PurgeMemory,
    FlushDns,
    ThinLocalSnapshots,
    ReindexSpotlight,
    CleanSystemCaches,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum HelperRequest {
    Ping,
    Status,
    SetFanAuto { fan: u8 },
    SetFanRpm { fan: u8, rpm: f32 },
    /// `None` disables the limit and lets the battery charge to 100 %.
    SetChargeLimit { percent: Option<u8> },
    SetForceDischarge { enabled: bool },
    Optimize { action: OptimizeAction },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HelperState {
    pub protocol_version: u32,
    pub helper_version: String,
    pub charge_limit: Option<u8>,
    pub charging_inhibited: bool,
    pub force_discharge: bool,
    pub manual_fans: Vec<u8>,
    pub supports_charge_limit: bool,
    pub supports_force_discharge: bool,
    pub supports_fan_control: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HelperResponse {
    pub ok: bool,
    pub message: String,
    pub state: Option<HelperState>,
}

impl HelperResponse {
    pub fn ok(message: impl Into<String>, state: Option<HelperState>) -> Self {
        Self { ok: true, message: message.into(), state }
    }
    pub fn err(message: impl Into<String>) -> Self {
        Self { ok: false, message: message.into(), state: None }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HelperStatus {
    pub installed: bool,
    pub reachable: bool,
    pub state: Option<HelperState>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Cloud storage (File Provider domains, iCloud Drive) and cloud backups
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CloudKind {
    ICloud,
    GoogleDrive,
    OneDrive,
    Dropbox,
    Box,
    ProtonDrive,
    /// Phones and other devices mounted through a File Provider (MacDroid…).
    Device,
    Other,
}

/// A cloud drive synced to this Mac. Detection is cheap; sizes come from
/// `CloudStats` (slow scan).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CloudProvider {
    /// Stable id: the folder name under ~/Library/CloudStorage, or "icloud".
    pub id: String,
    pub kind: CloudKind,
    pub name: String,
    pub account: Option<String>,
    pub root: String,
    /// Writable folder where backups go by default ("Mi unidad" in Google Drive).
    pub writable_root: Option<String>,
    pub accessible: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CloudStats {
    pub provider_id: String,
    /// Logical size of every file in the drive.
    pub total_bytes: u64,
    /// Bytes whose content is on this Mac (not "solo en la nube").
    pub downloaded_bytes: u64,
    /// Space actually allocated on the local disk.
    pub on_disk_bytes: u64,
    pub file_count: usize,
    pub folder_count: usize,
    pub cloud_only_count: usize,
    pub cloud_only_bytes: u64,
    /// Local changes not yet confirmed by the provider.
    pub pending_upload_count: usize,
    pub pending_upload_bytes: u64,
    pub uploading_count: usize,
    pub downloading_count: usize,
    pub conflict_count: usize,
    /// downloaded_bytes / total_bytes · 100.
    pub download_pct: f32,
    /// (total_bytes - pending_upload_bytes) / total_bytes · 100.
    pub upload_pct: f32,
    /// Folders that are only in the cloud and were not listed (size unknown).
    pub unlisted_folders: usize,
    pub skipped_errors: usize,
    /// A background scan is still counting (figures grow while it runs).
    pub scanning: bool,
    /// The last scan reached every listed folder.
    pub complete: bool,
    /// Upload status is checked for files changed in the last N days; older
    /// files are counted as uploaded.
    pub status_window_days: u32,
    pub scanned_at: u64,
    pub scan_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CloudItemState {
    /// Fully on this Mac and in the cloud.
    Synced,
    /// Only a placeholder on this Mac.
    CloudOnly,
    /// Folder with some files local and some cloud-only.
    Partial,
    Downloading,
    Uploading,
    /// Changed locally, waiting for the provider to upload it.
    PendingUpload,
    Conflict,
    /// Not managed by the provider (status unavailable).
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CloudItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub downloaded_bytes: u64,
    pub file_count: usize,
    pub state: CloudItemState,
    pub download_pct: f32,
    pub upload_pct: f32,
    /// Seconds since the Unix epoch.
    pub modified: Option<u64>,
    /// Folder too big to measure in time: figures are a lower bound.
    pub partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CloudListing {
    pub provider_id: String,
    pub path: String,
    /// None at the provider root.
    pub parent: Option<String>,
    pub items: Vec<CloudItem>,
    pub skipped_errors: usize,
}

/// Result of a quick bulk action (free up local space).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CloudActionReport {
    pub files: usize,
    pub bytes: u64,
    /// Files skipped because they are not uploaded yet (never evicted).
    pub skipped_not_uploaded: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferKind {
    /// Bring cloud-only files to this Mac.
    Download,
    /// Copy a folder into a cloud drive, then wait for the upload.
    Backup,
    /// Copy a backup version back to this Mac.
    Restore,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferPhase {
    Preparing,
    Copying,
    Downloading,
    Uploading,
    Done,
    Failed,
    Cancelled,
}

impl TransferPhase {
    pub fn is_finished(self) -> bool {
        matches!(self, TransferPhase::Done | TransferPhase::Failed | TransferPhase::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transfer {
    pub id: u64,
    pub kind: TransferKind,
    pub phase: TransferPhase,
    pub label: String,
    pub provider_id: String,
    pub backup_id: Option<String>,
    pub source: String,
    pub dest: String,
    pub files_total: usize,
    pub files_done: usize,
    pub bytes_total: u64,
    pub bytes_done: u64,
    /// Local copy progress (backup/restore), 0-100.
    pub copy_pct: f32,
    /// Bytes confirmed by the provider as uploaded, 0-100.
    pub upload_pct: f32,
    /// Bytes present on this Mac, 0-100.
    pub download_pct: f32,
    pub bytes_per_sec: u64,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackupMode {
    /// One folder kept up to date: only new or changed files are copied.
    Mirror,
    /// A dated full copy per run, keeping the last `keep_versions`.
    Snapshot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackupSchedule {
    Manual,
    Hourly,
    Daily,
    Weekly,
}

impl BackupSchedule {
    pub fn interval_secs(self) -> Option<u64> {
        match self {
            BackupSchedule::Manual => None,
            BackupSchedule::Hourly => Some(3_600),
            BackupSchedule::Daily => Some(86_400),
            BackupSchedule::Weekly => Some(7 * 86_400),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupJob {
    pub id: String,
    pub name: String,
    pub source: String,
    pub provider_id: String,
    /// Absolute folder inside the provider that holds this job's backups.
    pub dest: String,
    pub mode: BackupMode,
    pub schedule: BackupSchedule,
    pub keep_versions: u32,
    /// File or folder names skipped anywhere in the tree (".git", "node_modules").
    pub excludes: Vec<String>,
    pub enabled: bool,
    pub created_at: u64,
    pub last_run: Option<u64>,
    pub last_ok: Option<bool>,
    pub last_message: Option<String>,
    pub last_bytes: u64,
    pub next_run: Option<u64>,
}

/// What the UI sends to create (`id: None`) or edit a backup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupDraft {
    pub id: Option<String>,
    pub name: String,
    pub source: String,
    pub provider_id: String,
    /// Folder inside the provider's writable root. Default "iclear Backups".
    pub dest_folder: Option<String>,
    pub mode: BackupMode,
    pub schedule: BackupSchedule,
    pub keep_versions: u32,
    pub excludes: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupVersion {
    pub path: String,
    pub label: String,
    pub created_at: Option<u64>,
    pub size_bytes: u64,
    pub file_count: usize,
    /// Bytes that are only in the cloud (restoring downloads them first).
    pub cloud_only_bytes: u64,
}

// ---------------------------------------------------------------------------
// Helpers shared by UI and backend
// ---------------------------------------------------------------------------

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_like_finder() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
        assert_eq!(format_bytes(250_000_000_000), "250 GB");
    }

    #[test]
    fn helper_request_wire_format() {
        let json = serde_json::to_string(&HelperRequest::SetFanRpm { fan: 0, rpm: 2000.0 }).unwrap();
        assert_eq!(json, r#"{"cmd":"set_fan_rpm","fan":0,"rpm":2000.0}"#);
        let back: HelperRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, HelperRequest::SetFanRpm { fan: 0, rpm: 2000.0 });
    }
}
