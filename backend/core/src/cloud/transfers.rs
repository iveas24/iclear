//! Long-running cloud work (downloads, backups, restores) with live progress.
//! Each transfer runs on its own thread and updates a shared `Transfer`
//! record that the UI polls with `list_transfers`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iclear_protocol::*;

use super::{now, resolve, ubiquity, walk_files};

/// Finished transfers kept for the history list.
const KEEP_FINISHED: usize = 30;
const POLL: Duration = Duration::from_millis(1000);
/// A download with no progress for this long is reported as stalled.
const DOWNLOAD_STALL: Duration = Duration::from_secs(30 * 60);

struct Entry {
    t: Transfer,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Transfers {
    list: Mutex<VecDeque<Entry>>,
    next: AtomicU64,
}

/// Handle given to the worker thread of one transfer.
#[derive(Clone)]
pub struct Handle {
    pub id: u64,
    owner: Arc<Transfers>,
    cancel: Arc<AtomicBool>,
    speed: Arc<Mutex<(Instant, u64)>>,
}

impl Handle {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn update(&self, f: impl FnOnce(&mut Transfer)) {
        let mut list = self.owner.list.lock().unwrap();
        if let Some(e) = list.iter_mut().find(|e| e.t.id == self.id) {
            f(&mut e.t);
        }
    }

    /// Sets `bytes_done` and recomputes the transfer speed (smoothed).
    pub fn progress(&self, bytes_done: u64, files_done: usize) {
        let rate = {
            let mut s = self.speed.lock().unwrap();
            let dt = s.0.elapsed().as_secs_f64();
            if dt >= 1.0 {
                let r = (bytes_done.saturating_sub(s.1) as f64 / dt) as u64;
                *s = (Instant::now(), bytes_done);
                Some(r)
            } else {
                None
            }
        };
        self.update(|t| {
            t.bytes_done = bytes_done;
            t.files_done = files_done;
            if let Some(r) = rate {
                t.bytes_per_sec = if t.bytes_per_sec == 0 { r } else { (t.bytes_per_sec + r) / 2 };
            }
        });
    }

    pub fn finish(&self, phase: TransferPhase, message: Option<String>) {
        let phase = if self.cancelled() && phase != TransferPhase::Done { TransferPhase::Cancelled } else { phase };
        self.update(|t| {
            t.phase = phase;
            t.finished_at = Some(now());
            t.bytes_per_sec = 0;
            if message.is_some() {
                t.message = message;
            }
        });
        self.owner.trim();
    }

    pub fn get(&self) -> Option<Transfer> {
        self.owner.list.lock().unwrap().iter().find(|e| e.t.id == self.id).map(|e| e.t.clone())
    }
}

impl Transfers {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { list: Mutex::new(VecDeque::new()), next: AtomicU64::new(1) })
    }

    pub fn start(
        self: &Arc<Self>,
        kind: TransferKind,
        label: String,
        provider_id: String,
        source: String,
        dest: String,
        backup_id: Option<String>,
    ) -> Handle {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let t = Transfer {
            id,
            kind,
            phase: TransferPhase::Preparing,
            label,
            provider_id,
            backup_id,
            source,
            dest,
            files_total: 0,
            files_done: 0,
            bytes_total: 0,
            bytes_done: 0,
            copy_pct: 0.0,
            upload_pct: 0.0,
            download_pct: 0.0,
            bytes_per_sec: 0,
            started_at: now(),
            finished_at: None,
            message: None,
        };
        self.list.lock().unwrap().push_front(Entry { t, cancel: cancel.clone() });
        Handle { id, owner: self.clone(), cancel, speed: Arc::new(Mutex::new((Instant::now(), 0))) }
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Transfer> {
        self.list.lock().unwrap().iter().map(|e| e.t.clone()).collect()
    }

    pub fn active_count(&self) -> usize {
        self.list.lock().unwrap().iter().filter(|e| !e.t.phase.is_finished()).count()
    }

    pub fn is_backup_running(&self, backup_id: &str) -> bool {
        self.list
            .lock()
            .unwrap()
            .iter()
            .any(|e| !e.t.phase.is_finished() && e.t.backup_id.as_deref() == Some(backup_id))
    }

    pub fn cancel(&self, id: u64) -> Result<(), String> {
        let list = self.list.lock().unwrap();
        let e = list.iter().find(|e| e.t.id == id).ok_or("Transferencia no encontrada")?;
        e.cancel.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub fn clear_finished(&self) {
        self.list.lock().unwrap().retain(|e| !e.t.phase.is_finished());
    }

    fn trim(&self) {
        let mut list = self.list.lock().unwrap();
        let mut finished = 0;
        list.retain(|e| {
            if e.t.phase.is_finished() {
                finished += 1;
                finished <= KEEP_FINISHED
            } else {
                true
            }
        });
    }
}

fn pct(done: u64, total: u64) -> f32 {
    if total == 0 {
        100.0
    } else {
        (done as f64 / total as f64 * 100.0).min(100.0) as f32
    }
}

/// Brings every cloud-only file under `path` to this Mac and tracks it until
/// all of them are local.
pub fn download(transfers: &Arc<Transfers>, home: &Path, path: &str) -> Result<Transfer, String> {
    let (prov, p) = resolve(home, path)?;
    let label = format!("Descargar {}", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| prov.name.clone()));
    let h = transfers.start(TransferKind::Download, label, prov.id, p.display().to_string(), p.display().to_string(), None);
    let worker = h.clone();
    std::thread::spawn(move || run_download(worker, p));
    h.get().ok_or_else(|| "No se pudo iniciar la descarga".into())
}

fn run_download(h: Handle, root: PathBuf) {
    let (files, _) = walk_files(&root, true, &[]);
    let total: u64 = files.iter().map(|f| f.size).sum();
    let already: u64 = files.iter().filter(|f| !f.dataless).map(|f| f.size).sum();
    let mut pending: Vec<(PathBuf, u64)> = files.into_iter().filter(|f| f.dataless).map(|f| (f.path, f.size)).collect();
    let files_total = pending.len();
    let pending_total: u64 = pending.iter().map(|f| f.1).sum();
    h.update(|t| {
        t.phase = TransferPhase::Downloading;
        t.files_total = files_total;
        t.bytes_total = pending_total;
        t.download_pct = pct(already, total);
    });
    if pending.is_empty() {
        h.update(|t| t.download_pct = 100.0);
        return h.finish(TransferPhase::Done, Some("Todo ya estaba en este Mac".into()));
    }

    let mut errors = Vec::new();
    for (p, _) in &pending {
        if let Err(e) = ubiquity::start_download(p) {
            // Providers that do not implement the request still download on read.
            let p = p.clone();
            std::thread::spawn(move || {
                use std::io::Read;
                if let Ok(mut f) = std::fs::File::open(&p) {
                    let _ = f.read(&mut [0u8; 1]);
                }
            });
            if errors.len() < 3 {
                errors.push(e);
            }
        }
        if h.cancelled() {
            return h.finish(TransferPhase::Cancelled, Some("Cancelada. Lo ya pedido puede seguir bajando.".into()));
        }
    }

    let mut done_bytes = 0u64;
    let mut done_files = 0usize;
    let mut last_progress = Instant::now();
    while !pending.is_empty() {
        std::thread::sleep(POLL);
        if h.cancelled() {
            return h.finish(TransferPhase::Cancelled, Some("Cancelada. Lo ya pedido puede seguir bajando.".into()));
        }
        let before = pending.len();
        let mut partial = 0u64;
        pending.retain(|(p, size)| match std::fs::symlink_metadata(p) {
            Ok(m) if !ubiquity::is_dataless(&m) => {
                done_bytes += size;
                done_files += 1;
                false
            }
            Ok(m) => {
                partial += ubiquity::on_disk(&m).min(*size);
                true
            }
            Err(_) => {
                // Deleted or renamed meanwhile: nothing left to download.
                done_files += 1;
                false
            }
        });
        if pending.len() != before || partial > 0 {
            last_progress = Instant::now();
        }
        let current = (done_bytes + partial).min(pending_total);
        h.progress(current, done_files);
        h.update(|t| t.download_pct = pct(already + current, total));
        if last_progress.elapsed() > DOWNLOAD_STALL {
            let msg = match errors.first() {
                Some(e) => format!("Sin avance en 30 min. {e}"),
                None => "Sin avance en 30 min. Revisa la conexión o la app del proveedor.".into(),
            };
            return h.finish(TransferPhase::Failed, Some(msg));
        }
    }
    h.update(|t| t.download_pct = 100.0);
    h.finish(TransferPhase::Done, None);
}

/// Waits until the provider confirms every file as uploaded, updating
/// `upload_pct`. Returns false when the wait was cancelled or timed out.
pub fn wait_uploaded(h: &Handle, files: Vec<(PathBuf, u64)>, max_wait: Duration) -> bool {
    let total: u64 = files.iter().map(|f| f.1).sum();
    let mut pending = files;
    let mut done = 0u64;
    let started = Instant::now();
    let mut first = true;
    loop {
        pending.retain(|(p, size)| match ubiquity::uploaded(p) {
            Some(false) => true,
            // Uploaded, deleted, or the provider does not report status.
            _ => {
                done += size;
                false
            }
        });
        h.update(|t| t.upload_pct = pct(done, total));
        if pending.is_empty() {
            return true;
        }
        if h.cancelled() || started.elapsed() > max_wait {
            return false;
        }
        if first {
            h.update(|t| t.phase = TransferPhase::Uploading);
            first = false;
        }
        std::thread::sleep(POLL * 2);
    }
}
