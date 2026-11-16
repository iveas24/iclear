//! Cloud drives synced to this Mac: detection, sync status per file and
//! folder, download / free-up-space actions, transfers with progress and
//! backups into any of the drives. See docs/CLOUD.md.

pub mod backup;
pub mod transfers;
pub mod ubiquity;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iclear_protocol::*;
use rayon::prelude::*;

pub use backup::BackupService;
pub use transfers::Transfers;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn mtime(meta: &fs::Metadata) -> Option<u64> {
    meta.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

const ICLOUD_ID: &str = "icloud";
/// Writable folders inside a drive whose root is read-only (Google Drive).
const MY_DRIVE: &[&str] = &["Mi unidad", "My Drive", "Mon Drive", "Meine Ablage", "Il mio Drive", "Meu Drive"];

pub fn cloud_storage_dir(home: &Path) -> PathBuf {
    home.join("Library/CloudStorage")
}

pub fn icloud_root(home: &Path) -> PathBuf {
    home.join("Library/Mobile Documents/com~apple~CloudDocs")
}

fn kind_and_name(prefix: &str) -> (CloudKind, String) {
    match prefix {
        "GoogleDrive" => (CloudKind::GoogleDrive, "Google Drive".into()),
        "OneDrive" => (CloudKind::OneDrive, "OneDrive".into()),
        "Dropbox" => (CloudKind::Dropbox, "Dropbox".into()),
        "Box" => (CloudKind::Box, "Box".into()),
        "ProtonDrive" => (CloudKind::ProtonDrive, "Proton Drive".into()),
        "iCloudDrive" => (CloudKind::ICloud, "iCloud Drive".into()),
        "MacDroid" => (CloudKind::Device, "MacDroid".into()),
        other => {
            // "pCloudDrive" -> "p Cloud Drive"-ish: split on case changes.
            let mut name = String::new();
            for (i, c) in other.chars().enumerate() {
                if i > 0 && c.is_uppercase() && !name.ends_with(' ') {
                    name.push(' ');
                }
                name.push(c);
            }
            (CloudKind::Other, name)
        }
    }
}

fn writable(path: &Path) -> bool {
    let Some(c) = path.to_str().and_then(|s| std::ffi::CString::new(s).ok()) else { return false };
    // SAFETY: valid NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

fn writable_root(root: &Path) -> Option<PathBuf> {
    if writable(root) {
        return Some(root.to_path_buf());
    }
    for name in MY_DRIVE {
        let p = root.join(name);
        if p.is_dir() && writable(&p) {
            return Some(p);
        }
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .filter(|p| p.is_dir() && writable(p))
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

fn provider_from_dir(dir_name: &str, root: PathBuf) -> CloudProvider {
    let (prefix, rest) = dir_name.split_once('-').unwrap_or((dir_name, ""));
    let (kind, name) = kind_and_name(prefix);
    let account = rest.trim_end_matches("-folder").trim();
    let account = if let Some(rest) = account.strip_prefix("iCloudDrive (").and_then(|r| r.strip_suffix(')')) {
        // Local copy macOS keeps after signing out of iCloud.
        Some(format!("Copia local del {rest}"))
    } else {
        (!account.is_empty() && account != "iCloudDrive").then(|| account.to_string())
    };
    let accessible = fs::read_dir(&root).is_ok();
    CloudProvider {
        id: dir_name.to_string(),
        kind,
        name,
        account,
        writable_root: if accessible { writable_root(&root).map(|p| p.display().to_string()) } else { None },
        root: root.display().to_string(),
        accessible,
    }
}

/// Every drive synced to this Mac. Cheap: no sizes.
pub fn providers(home: &Path) -> Vec<CloudProvider> {
    let mut out = Vec::new();
    let icloud = icloud_root(home);
    if icloud.is_dir() {
        let accessible = fs::read_dir(&icloud).is_ok();
        out.push(CloudProvider {
            id: ICLOUD_ID.into(),
            kind: CloudKind::ICloud,
            name: "iCloud Drive".into(),
            account: None,
            root: icloud.display().to_string(),
            writable_root: accessible.then(|| icloud.display().to_string()),
            accessible,
        });
    }
    if let Ok(rd) = fs::read_dir(cloud_storage_dir(home)) {
        let mut found: Vec<CloudProvider> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                (!name.starts_with('.')).then(|| provider_from_dir(&name, e.path()))
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        out.extend(found);
    }
    out
}

pub fn provider(home: &Path, id: &str) -> Result<CloudProvider, String> {
    providers(home).into_iter().find(|p| p.id == id).ok_or_else(|| format!("No se encontró la nube «{id}»"))
}

/// Resolves `path` (absolute) and checks it is inside a known drive.
pub fn resolve(home: &Path, path: &str) -> Result<(CloudProvider, PathBuf), String> {
    let p = PathBuf::from(path);
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("Ruta no válida".into());
    }
    for prov in providers(home) {
        if p.starts_with(&prov.root) {
            return Ok((prov, p));
        }
    }
    Err("La ruta no está dentro de una nube sincronizada".into())
}

// ---------------------------------------------------------------------------
// Tree aggregation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct Agg {
    pub total: u64,
    pub downloaded: u64,
    pub on_disk: u64,
    pub files: usize,
    pub folders: usize,
    pub cloud_only: usize,
    pub cloud_only_bytes: u64,
    pub pending: usize,
    pub pending_bytes: u64,
    pub uploading: usize,
    pub downloading: usize,
    pub conflicts: usize,
    pub unmanaged: usize,
    pub unlisted: usize,
    pub errors: usize,
    pub newest: u64,
}

impl Agg {
    fn merge(mut self, o: Agg) -> Agg {
        self.total += o.total;
        self.downloaded += o.downloaded;
        self.on_disk += o.on_disk;
        self.files += o.files;
        self.folders += o.folders;
        self.cloud_only += o.cloud_only;
        self.cloud_only_bytes += o.cloud_only_bytes;
        self.pending += o.pending;
        self.pending_bytes += o.pending_bytes;
        self.uploading += o.uploading;
        self.downloading += o.downloading;
        self.conflicts += o.conflicts;
        self.unmanaged += o.unmanaged;
        self.unlisted += o.unlisted;
        self.errors += o.errors;
        self.newest = self.newest.max(o.newest);
        self
    }

    pub fn download_pct(&self) -> f32 {
        if self.total == 0 {
            return if self.cloud_only > 0 || self.unlisted > 0 { 0.0 } else { 100.0 };
        }
        (self.downloaded as f64 / self.total as f64 * 100.0) as f32
    }

    pub fn upload_pct(&self) -> f32 {
        if self.total == 0 {
            return if self.pending > 0 { 0.0 } else { 100.0 };
        }
        ((self.total - self.pending_bytes.min(self.total)) as f64 / self.total as f64 * 100.0) as f32
    }

    fn state(&self) -> CloudItemState {
        if self.conflicts > 0 {
            CloudItemState::Conflict
        } else if self.uploading > 0 {
            CloudItemState::Uploading
        } else if self.downloading > 0 {
            CloudItemState::Downloading
        } else if self.pending > 0 {
            CloudItemState::PendingUpload
        } else if self.files > 0 && self.unmanaged == self.files {
            CloudItemState::Unknown
        } else if (self.files > 0 && self.cloud_only == self.files) || (self.files == 0 && self.unlisted > 0) {
            CloudItemState::CloudOnly
        } else if self.cloud_only > 0 || self.unlisted > 0 {
            CloudItemState::Partial
        } else {
            CloudItemState::Synced
        }
    }
}

fn skip_name(name: &str, at_root: bool) -> bool {
    name == ".DS_Store" || (at_root && name.starts_with('.'))
}

/// Upload status is only asked for files changed in this window: anything
/// pending upload is a recent change, and asking the provider about every
/// file of a drive with a million files would take hours.
pub const STATUS_WINDOW_DAYS: u32 = 14;

/// Shared state of one tree walk.
pub struct Ctx {
    /// Files changed after this (epoch secs) get their upload status checked.
    status_since: u64,
    deadline: Option<Instant>,
    cancel: Arc<AtomicBool>,
    /// Running totals for live progress (direct files of each folder).
    live: Option<Arc<Mutex<Agg>>>,
    /// Subtree totals of every finished folder.
    dirs: Option<Arc<Mutex<HashMap<PathBuf, Agg>>>>,
    truncated: AtomicBool,
}

impl Ctx {
    pub fn quick(budget: Duration) -> Self {
        Self {
            status_since: 0,
            deadline: Some(Instant::now() + budget),
            cancel: Arc::new(AtomicBool::new(false)),
            live: None,
            dirs: None,
            truncated: AtomicBool::new(false),
        }
    }

    fn stop(&self) -> bool {
        if self.cancel.load(Ordering::Relaxed) || self.deadline.is_some_and(|d| Instant::now() > d) {
            self.truncated.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn truncated(&self) -> bool {
        self.truncated.load(Ordering::Relaxed)
    }
}

/// Stats for one file. `status` asks the provider for upload/download flags.
fn file_agg(path: &Path, meta: &fs::Metadata, status: bool) -> Agg {
    let size = meta.len();
    let mut a = Agg { total: size, files: 1, on_disk: ubiquity::on_disk(meta), newest: mtime(meta).unwrap_or(0), ..Agg::default() };
    if ubiquity::is_dataless(meta) {
        a.cloud_only = 1;
        a.cloud_only_bytes = size;
        if status && ubiquity::flags(path).downloading {
            a.downloading = 1;
        }
        return a;
    }
    a.downloaded = size;
    if status {
        let f = ubiquity::flags(path);
        if !f.managed {
            a.unmanaged = 1;
        } else {
            if !f.uploaded {
                a.pending = 1;
                a.pending_bytes = size;
            }
            a.uploading = f.uploading as usize;
            a.downloading = f.downloading as usize;
            a.conflicts = f.conflict as usize;
        }
    }
    a
}

/// Recursive stats. Cloud-only folders are not listed (that would make the
/// provider fetch them from the network): they count as `unlisted`.
pub fn scan_dir(dir: &Path, ctx: &Ctx, at_root: bool) -> Agg {
    if ctx.stop() {
        return Agg::default();
    }
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd.flatten().collect::<Vec<_>>(),
        Err(_) => return Agg { errors: 1, ..Agg::default() },
    };
    let mut direct = Agg::default();
    let mut subdirs = Vec::new();
    for e in entries {
        let name = e.file_name();
        if skip_name(&name.to_string_lossy(), at_root) {
            continue;
        }
        let path = e.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            direct.errors += 1;
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            direct.folders += 1;
            if ubiquity::is_dataless(&meta) {
                direct.unlisted += 1;
            } else {
                subdirs.push(path);
            }
        } else {
            let recent = mtime(&meta).is_some_and(|m| m >= ctx.status_since);
            direct = direct.merge(file_agg(&path, &meta, recent));
        }
    }
    if let Some(live) = &ctx.live {
        let mut l = live.lock().unwrap();
        *l = l.merge(direct);
    }
    let sub = subdirs.par_iter().map(|d| scan_dir(d, ctx, false)).reduce(Agg::default, Agg::merge);
    let total = direct.merge(sub);
    if let Some(dirs) = &ctx.dirs {
        if !ctx.truncated() {
            dirs.lock().unwrap().insert(dir.to_path_buf(), total);
        }
    }
    total
}

/// Convenience for tests and small folders: full scan with status for every file.
pub fn scan_tree(dir: &Path, with_status: bool, at_root: bool) -> Agg {
    let mut ctx = Ctx::quick(Duration::from_secs(24 * 3600));
    ctx.status_since = if with_status { 0 } else { u64::MAX };
    scan_dir(dir, &ctx, at_root)
}

// ---------------------------------------------------------------------------
// Background scans with cache
// ---------------------------------------------------------------------------

struct ProviderScan {
    live: Arc<Mutex<Agg>>,
    dirs: Arc<Mutex<HashMap<PathBuf, Agg>>>,
    cancel: Arc<AtomicBool>,
    running: bool,
    complete: bool,
    started: Instant,
    finished_at: u64,
    scan_ms: u64,
}

/// Keeps one background scan per drive and its per-folder totals.
#[derive(Default)]
pub struct Scans {
    map: Mutex<HashMap<String, ProviderScan>>,
}

/// Longest a full drive scan may run.
const SCAN_LIMIT: Duration = Duration::from_secs(20 * 60);
/// Time budget to measure the folders of one listing without cache.
const LIST_BUDGET: Duration = Duration::from_millis(2500);

impl Scans {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Starts (or restarts with `force`) the scan of one drive.
    pub fn start(self: &Arc<Self>, home: &Path, id: &str, force: bool) -> Result<(), String> {
        let prov = provider(home, id)?;
        if !prov.accessible {
            return Err(format!("Sin acceso a {}. Concede Acceso total al disco a iclear.", prov.name));
        }
        let mut map = self.map.lock().unwrap();
        if let Some(s) = map.get(id) {
            if s.running || !force {
                return Ok(());
            }
        }
        let scan = ProviderScan {
            live: Arc::new(Mutex::new(Agg::default())),
            dirs: Arc::new(Mutex::new(HashMap::new())),
            cancel: Arc::new(AtomicBool::new(false)),
            running: true,
            complete: false,
            started: Instant::now(),
            finished_at: 0,
            scan_ms: 0,
        };
        let ctx = Ctx {
            status_since: now().saturating_sub(STATUS_WINDOW_DAYS as u64 * 86_400),
            deadline: Some(Instant::now() + SCAN_LIMIT),
            cancel: scan.cancel.clone(),
            live: Some(scan.live.clone()),
            dirs: Some(scan.dirs.clone()),
            truncated: AtomicBool::new(false),
        };
        if let Some(old) = map.insert(id.to_string(), scan) {
            old.cancel.store(true, Ordering::Relaxed);
        }
        drop(map);
        let me = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            let root = PathBuf::from(&prov.root);
            scan_dir(&root, &ctx, true);
            let mut map = me.map.lock().unwrap();
            if let Some(s) = map.get_mut(&id) {
                if Arc::ptr_eq(&s.cancel, &ctx.cancel) {
                    s.running = false;
                    s.complete = !ctx.truncated();
                    s.finished_at = now();
                    s.scan_ms = s.started.elapsed().as_millis() as u64;
                }
            }
        });
        Ok(())
    }

    /// Current figures of a drive; starts the first scan automatically.
    pub fn stats(self: &Arc<Self>, home: &Path, id: &str) -> Result<CloudStats, String> {
        if !self.map.lock().unwrap().contains_key(id) {
            self.start(home, id, false)?;
        }
        let map = self.map.lock().unwrap();
        let s = map.get(id).ok_or("Escaneo no disponible")?;
        let a = *s.live.lock().unwrap();
        Ok(CloudStats {
            provider_id: id.to_string(),
            total_bytes: a.total,
            downloaded_bytes: a.downloaded,
            on_disk_bytes: a.on_disk,
            file_count: a.files,
            folder_count: a.folders,
            cloud_only_count: a.cloud_only,
            cloud_only_bytes: a.cloud_only_bytes,
            pending_upload_count: a.pending,
            pending_upload_bytes: a.pending_bytes,
            uploading_count: a.uploading,
            downloading_count: a.downloading,
            conflict_count: a.conflicts,
            download_pct: a.download_pct(),
            upload_pct: a.upload_pct(),
            unlisted_folders: a.unlisted,
            skipped_errors: a.errors,
            scanning: s.running,
            complete: s.complete,
            status_window_days: STATUS_WINDOW_DAYS,
            scanned_at: s.finished_at,
            scan_ms: if s.running { s.started.elapsed().as_millis() as u64 } else { s.scan_ms },
        })
    }

    fn cached(&self, id: &str, dir: &Path) -> Option<Agg> {
        let map = self.map.lock().unwrap();
        let s = map.get(id)?;
        let a = s.dirs.lock().unwrap().get(dir).copied();
        a
    }

    pub fn cancel_all(&self) {
        for s in self.map.lock().unwrap().values() {
            s.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// Full stats of one drive, blocking (CLI).
pub fn stats(home: &Path, id: &str, limit: Duration) -> Result<CloudStats, String> {
    let scans = Scans::new();
    scans.start(home, id, true)?;
    let t = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let s = scans.stats(home, id)?;
        if !s.scanning || t.elapsed() > limit {
            if s.scanning {
                scans.cancel_all();
            }
            return Ok(s);
        }
        eprint!("\r{} archivos…", s.file_count);
    }
}

fn item_from(path: &Path, name: String, meta: &fs::Metadata, cached: Option<Agg>, ctx: &Ctx) -> CloudItem {
    let is_dir = meta.is_dir();
    let mut partial = false;
    let a = if !is_dir {
        file_agg(path, meta, true)
    } else if ubiquity::is_dataless(meta) {
        Agg { unlisted: 1, ..Agg::default() }
    } else if let Some(a) = cached {
        a
    } else {
        let sub = Ctx { cancel: ctx.cancel.clone(), deadline: ctx.deadline, status_since: ctx.status_since, live: None, dirs: None, truncated: AtomicBool::new(false) };
        let a = scan_dir(path, &sub, false);
        partial = sub.truncated();
        a
    };
    let mut state = a.state();
    if !is_dir && state == CloudItemState::Synced && a.unmanaged == 1 {
        state = CloudItemState::Unknown;
    }
    CloudItem {
        name,
        path: path.display().to_string(),
        is_dir,
        size_bytes: a.total,
        downloaded_bytes: a.downloaded,
        file_count: a.files,
        state,
        download_pct: a.download_pct(),
        upload_pct: a.upload_pct(),
        modified: if is_dir { (a.newest > 0).then_some(a.newest).or_else(|| mtime(meta)) } else { mtime(meta) },
        partial,
    }
}

/// One folder of a drive with per-item sync state. `path: None` = root.
/// Folder sizes come from the drive's background scan when available.
pub fn list(home: &Path, scans: &Scans, id: &str, path: Option<&str>) -> Result<CloudListing, String> {
    let prov = provider(home, id)?;
    let root = PathBuf::from(&prov.root);
    let dir = match path.filter(|p| !p.is_empty()) {
        Some(p) => {
            let (owner, p) = resolve(home, p)?;
            if owner.id != prov.id {
                return Err("La carpeta no pertenece a esta nube".into());
            }
            p
        }
        None => root.clone(),
    };
    let at_root = dir == root;
    let entries = fs::read_dir(&dir).map_err(|e| format!("No se pudo abrir {}: {e}", dir.display()))?;
    let entries: Vec<_> = entries.flatten().collect();
    let mut ctx = Ctx::quick(LIST_BUDGET);
    ctx.status_since = now().saturating_sub(STATUS_WINDOW_DAYS as u64 * 86_400);
    let results: Vec<Result<CloudItem, ()>> = entries
        .par_iter()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if skip_name(&name, at_root) {
                return None;
            }
            let path = e.path();
            let Ok(meta) = fs::symlink_metadata(&path) else { return Some(Err(())) };
            if meta.file_type().is_symlink() {
                return None;
            }
            let cached = if meta.is_dir() { scans.cached(&prov.id, &path) } else { None };
            Some(Ok(item_from(&path, name, &meta, cached, &ctx)))
        })
        .collect();
    let skipped = results.iter().filter(|r| r.is_err()).count();
    let mut items: Vec<CloudItem> = results.into_iter().flatten().collect();
    items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(b.size_bytes.cmp(&a.size_bytes)).then(a.name.cmp(&b.name)));
    Ok(CloudListing {
        provider_id: prov.id,
        path: dir.display().to_string(),
        parent: (!at_root).then(|| dir.parent().map(|p| p.display().to_string())).flatten(),
        items,
        skipped_errors: skipped,
    })
}

// ---------------------------------------------------------------------------
// Bulk file walks for actions
// ---------------------------------------------------------------------------

pub struct WalkFile {
    pub path: PathBuf,
    pub size: u64,
    pub dataless: bool,
    pub mtime: Option<SystemTime>,
}

/// Every regular file under `path` (or `path` itself). `list_dataless_dirs`
/// lets the provider fetch folder listings from the network (downloads).
pub fn walk_files(path: &Path, list_dataless_dirs: bool, excludes: &[String]) -> (Vec<WalkFile>, usize) {
    let mut out = Vec::new();
    let mut errors = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&p) else {
            errors += 1;
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if ubiquity::is_dataless(&meta) && !list_dataless_dirs {
                continue;
            }
            match fs::read_dir(&p) {
                Ok(rd) => {
                    for e in rd.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name == ".DS_Store" || excludes.iter().any(|x| x == &name) {
                            continue;
                        }
                        stack.push(e.path());
                    }
                }
                Err(_) => errors += 1,
            }
        } else if meta.is_file() {
            out.push(WalkFile { size: meta.len(), dataless: ubiquity::is_dataless(&meta), mtime: meta.modified().ok(), path: p });
        }
    }
    (out, errors)
}

/// Frees local space: evicts every downloaded and already-uploaded file under
/// `path`. Files with pending changes are skipped, never evicted.
pub fn evict(home: &Path, path: &str) -> Result<CloudActionReport, String> {
    let (_, p) = resolve(home, path)?;
    let (files, _) = walk_files(&p, false, &[]);
    let results: Vec<(u64, Result<bool, String>)> = files
        .par_iter()
        .filter(|f| !f.dataless)
        .map(|f| {
            let r = match ubiquity::uploaded(&f.path) {
                Some(true) => ubiquity::evict(&f.path).map(|_| true).map_err(|e| format!("{}: {e}", f.path.display())),
                Some(false) => Ok(false),
                None => Err(format!("{}: el proveedor no permite liberar este archivo", f.path.display())),
            };
            (f.size, r)
        })
        .collect();
    let mut report = CloudActionReport::default();
    for (size, r) in results {
        match r {
            Ok(true) => {
                report.files += 1;
                report.bytes += size;
            }
            Ok(false) => report.skipped_not_uploaded += 1,
            Err(e) if report.errors.len() < 5 => report.errors.push(e),
            Err(_) => {}
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_folder_names() {
        let p = provider_from_dir("GoogleDrive-ana@example.com", PathBuf::from("/nonexistent"));
        assert_eq!(p.kind, CloudKind::GoogleDrive);
        assert_eq!(p.name, "Google Drive");
        assert_eq!(p.account.as_deref(), Some("ana@example.com"));
        assert!(!p.accessible);

        let p = provider_from_dir("ProtonDrive-ana@proton.me-folder", PathBuf::from("/nonexistent"));
        assert_eq!(p.kind, CloudKind::ProtonDrive);
        assert_eq!(p.account.as_deref(), Some("ana@proton.me"));

        let p = provider_from_dir("pCloudDrive", PathBuf::from("/nonexistent"));
        assert_eq!(p.kind, CloudKind::Other);
        assert_eq!(p.account, None);
    }

    #[test]
    fn resolve_rejects_paths_outside_drives() {
        let home = tempfile::tempdir().unwrap();
        let drive = home.path().join("Library/CloudStorage/Dropbox");
        fs::create_dir_all(drive.join("docs")).unwrap();
        let ok = resolve(home.path(), drive.join("docs").to_str().unwrap()).unwrap();
        assert_eq!(ok.0.kind, CloudKind::Dropbox);
        assert!(resolve(home.path(), "/etc").is_err());
        assert!(resolve(home.path(), &format!("{}/../../..", drive.display())).is_err());
        assert!(resolve(home.path(), "relative/path").is_err());
    }

    #[test]
    fn aggregates_plain_folder_as_local() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), vec![0u8; 1000]).unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), vec![0u8; 500]).unwrap();
        fs::write(dir.path().join(".DS_Store"), b"x").unwrap();
        let a = scan_tree(dir.path(), true, true);
        assert_eq!(a.files, 2);
        assert_eq!(a.total, 1500);
        assert_eq!(a.downloaded, 1500);
        assert_eq!(a.download_pct(), 100.0);
        assert_eq!(a.state(), CloudItemState::Unknown);
    }
}
