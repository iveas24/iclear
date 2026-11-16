//! Backups of local folders into any synced cloud drive.
//!
//! iclear copies the files into the drive's folder and then waits until the
//! provider confirms the upload, so a backup reports two progresses: local
//! copy and upload. Jobs live in
//! ~/Library/Application Support/iclear/cloud-backups.json and runs are logged
//! to ~/Library/Logs/iclear/backups.log.
//!
//! Safety rules:
//! - Never overwrites or deletes anything in the source.
//! - Restores always go to a new folder, never over existing files.
//! - The only deletions are old snapshot folders of the same job, inside the
//!   job's destination and named with iclear's timestamp format.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iclear_protocol::*;

use super::transfers::{wait_uploaded, Handle, Transfers};
use super::{icloud_root, cloud_storage_dir, now, provider, scan_tree, walk_files};

const DEFAULT_FOLDER: &str = "iclear Backups";
const IN_PROGRESS: &str = " (en curso)";
const CHUNK: usize = 1 << 20;
/// How long a run waits for the provider to confirm the upload.
const UPLOAD_WAIT: Duration = Duration::from_secs(6 * 3600);
pub const DEFAULT_EXCLUDES: &[&str] = &[".git", "node_modules", "target", ".Trash", ".cache"];

pub struct BackupService {
    home: PathBuf,
    file: PathBuf,
    jobs: Mutex<Vec<BackupJob>>,
    transfers: Arc<Transfers>,
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '\\') || c.is_control() { '-' } else { c })
        .collect();
    let s = s.trim().trim_start_matches('.').trim().to_string();
    if s.is_empty() {
        "Backup".into()
    } else {
        s.chars().take(80).collect()
    }
}

fn new_id() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("bk{:x}", n)
}

// ---- local time stamps ----------------------------------------------------

/// "2026-11-15 16.30.05" in local time (Finder-safe: no colons).
pub fn stamp(ts: u64) -> String {
    let t = ts as libc::time_t;
    // SAFETY: localtime_r writes into the provided struct.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}.{:02}.{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// Inverse of `stamp`; None for any other name.
pub fn parse_stamp(name: &str) -> Option<u64> {
    let b = name.as_bytes();
    if b.len() != 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b' ' || b[13] != b'.' || b[16] != b'.' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| name.get(r)?.parse::<i32>().ok();
    // SAFETY: zeroed tm is a valid starting value for mktime.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = n(0..4)? - 1900;
    tm.tm_mon = n(5..7)? - 1;
    tm.tm_mday = n(8..10)?;
    tm.tm_hour = n(11..13)?;
    tm.tm_min = n(14..16)?;
    tm.tm_sec = n(17..19)?;
    tm.tm_isdst = -1;
    let t = unsafe { libc::mktime(&mut tm) };
    (t >= 0).then_some(t as u64)
}

// ---- service --------------------------------------------------------------

impl BackupService {
    pub fn new(home: PathBuf, transfers: Arc<Transfers>) -> Arc<Self> {
        let home = fs::canonicalize(&home).unwrap_or(home);
        let file = home.join("Library/Application Support/iclear/cloud-backups.json");
        let jobs = fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Arc::new(Self { home, file, jobs: Mutex::new(jobs), transfers })
    }

    fn save(&self, jobs: &[BackupJob]) -> Result<(), String> {
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.file.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(jobs).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.file).map_err(|e| e.to_string())
    }

    fn log(&self, line: &str) {
        let dir = self.home.join("Library/Logs/iclear");
        let _ = fs::create_dir_all(&dir);
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join("backups.log")) {
            let _ = writeln!(f, "{} {line}", stamp(now()));
        }
    }

    fn with_next_run(mut j: BackupJob) -> BackupJob {
        j.next_run = match (j.enabled, j.schedule.interval_secs()) {
            (true, Some(i)) => Some(j.last_run.map(|l| l + i).unwrap_or(j.created_at + 60)),
            _ => None,
        };
        j
    }

    pub fn list(&self) -> Vec<BackupJob> {
        self.jobs.lock().unwrap().iter().cloned().map(Self::with_next_run).collect()
    }

    fn get(&self, id: &str) -> Result<BackupJob, String> {
        self.jobs.lock().unwrap().iter().find(|j| j.id == id).cloned().ok_or_else(|| "Backup no encontrado".into())
    }

    fn validate_source(&self, source: &str, dest: &Path) -> Result<PathBuf, String> {
        let p = PathBuf::from(source.trim());
        let p = if let Ok(rest) = p.strip_prefix("~") { self.home.join(rest) } else { p };
        if !p.is_absolute() {
            return Err("La carpeta de origen debe ser una ruta completa".into());
        }
        let p = fs::canonicalize(&p).map_err(|_| format!("No existe la carpeta {}", p.display()))?;
        if !p.is_dir() {
            return Err("El origen debe ser una carpeta".into());
        }
        if p == Path::new("/") || p == self.home {
            return Err("Elige una carpeta concreta (Documentos, Escritorio…), no todo el disco ni toda tu carpeta de usuario".into());
        }
        let cloud_dirs = [cloud_storage_dir(&self.home), icloud_root(&self.home)];
        if dest.starts_with(&p) || cloud_dirs.iter().any(|c| c.starts_with(&p)) {
            return Err("El origen no puede contener la carpeta de destino ni las nubes".into());
        }
        if p.starts_with(dest) {
            return Err("El origen no puede estar dentro del destino del backup".into());
        }
        Ok(p)
    }

    fn dest_for(&self, draft: &BackupDraft, own_id: Option<&str>) -> Result<PathBuf, String> {
        let prov = provider(&self.home, &draft.provider_id)?;
        let base = prov.writable_root.ok_or_else(|| format!("{} no tiene una carpeta con permiso de escritura", prov.name))?;
        let folder = draft.dest_folder.as_deref().map(str::trim).filter(|f| !f.is_empty()).unwrap_or(DEFAULT_FOLDER);
        let mut dir = PathBuf::from(base);
        for part in folder.split('/').filter(|s| !s.is_empty() && *s != "." && *s != "..") {
            dir.push(sanitize(part));
        }
        let name = sanitize(&draft.name);
        let taken: HashSet<PathBuf> = self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .filter(|j| Some(j.id.as_str()) != own_id)
            .map(|j| PathBuf::from(&j.dest))
            .collect();
        let mut dest = dir.join(&name);
        let mut n = 2;
        while taken.contains(&dest) {
            dest = dir.join(format!("{name} ({n})"));
            n += 1;
        }
        Ok(dest)
    }

    /// Creates or edits a job. Editing keeps the destination unless the
    /// provider, folder or name changed.
    pub fn save_job(&self, draft: BackupDraft) -> Result<BackupJob, String> {
        let name = draft.name.trim();
        if name.is_empty() {
            return Err("Ponle un nombre al backup".into());
        }
        let existing = match &draft.id {
            Some(id) => Some(self.get(id)?),
            None => None,
        };
        let dest = match &existing {
            Some(j) if j.provider_id == draft.provider_id && j.name == name && draft.dest_folder.is_none() => PathBuf::from(&j.dest),
            _ => self.dest_for(&draft, draft.id.as_deref())?,
        };
        let source = self.validate_source(&draft.source, &dest)?;
        let excludes: Vec<String> = draft
            .excludes
            .iter()
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty() && !e.contains('/'))
            .collect();
        let job = BackupJob {
            id: existing.as_ref().map(|j| j.id.clone()).unwrap_or_else(new_id),
            name: name.chars().take(80).collect(),
            source: source.display().to_string(),
            provider_id: draft.provider_id,
            dest: dest.display().to_string(),
            mode: draft.mode,
            schedule: draft.schedule,
            keep_versions: draft.keep_versions.clamp(1, 100),
            excludes,
            enabled: draft.enabled,
            created_at: existing.as_ref().map(|j| j.created_at).unwrap_or_else(now),
            last_run: existing.as_ref().and_then(|j| j.last_run),
            last_ok: existing.as_ref().and_then(|j| j.last_ok),
            last_message: existing.as_ref().and_then(|j| j.last_message.clone()),
            last_bytes: existing.as_ref().map(|j| j.last_bytes).unwrap_or(0),
            next_run: None,
        };
        let mut jobs = self.jobs.lock().unwrap();
        match jobs.iter_mut().find(|j| j.id == job.id) {
            Some(slot) => *slot = job.clone(),
            None => jobs.push(job.clone()),
        }
        self.save(&jobs)?;
        Ok(Self::with_next_run(job))
    }

    /// Forgets a job. The copies already in the cloud are kept.
    pub fn delete_job(&self, id: &str) -> Result<(), String> {
        if self.transfers.is_backup_running(id) {
            return Err("El backup se está ejecutando; cancélalo primero".into());
        }
        let mut jobs = self.jobs.lock().unwrap();
        let before = jobs.len();
        jobs.retain(|j| j.id != id);
        if jobs.len() == before {
            return Err("Backup no encontrado".into());
        }
        self.save(&jobs)
    }

    fn record(&self, id: &str, ok: bool, message: String, bytes: u64, started: u64) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
            j.last_run = Some(started);
            j.last_ok = Some(ok);
            j.last_message = Some(message);
            j.last_bytes = bytes;
        }
        let _ = self.save(&jobs);
    }

    /// Starts a run now (manual or scheduled).
    pub fn run(self: &Arc<Self>, id: &str) -> Result<Transfer, String> {
        let job = self.get(id)?;
        if self.transfers.is_backup_running(id) {
            return Err("Este backup ya se está ejecutando".into());
        }
        let h = self.transfers.start(
            TransferKind::Backup,
            format!("Backup «{}»", job.name),
            job.provider_id.clone(),
            job.source.clone(),
            job.dest.clone(),
            Some(job.id.clone()),
        );
        let svc = self.clone();
        let worker = h.clone();
        std::thread::spawn(move || svc.run_job(worker, job));
        h.get().ok_or_else(|| "No se pudo iniciar el backup".into())
    }

    /// Called every minute by the app: starts scheduled jobs that are due.
    pub fn tick(self: &Arc<Self>) {
        let t = now();
        let due: Vec<String> = self
            .list()
            .into_iter()
            .filter(|j| j.next_run.is_some_and(|n| n <= t) && !self.transfers.is_backup_running(&j.id))
            .map(|j| j.id)
            .collect();
        for id in due {
            let _ = self.run(&id);
        }
    }

    fn run_job(&self, h: Handle, job: BackupJob) {
        let started = now();
        let result = self.copy_job(&h, &job);
        let (ok, msg, bytes) = match result {
            Ok((bytes, msg)) => (true, msg, bytes),
            Err(e) => (false, e, 0),
        };
        if h.cancelled() {
            self.record(&job.id, false, "Cancelado".into(), bytes, started);
            self.log(&format!("[{}] cancelado", job.name));
            return h.finish(TransferPhase::Cancelled, Some("Cancelado".into()));
        }
        self.log(&format!("[{}] {} — {}", job.name, if ok { "ok" } else { "error" }, msg));
        self.record(&job.id, ok, msg.clone(), bytes, started);
        h.finish(if ok { TransferPhase::Done } else { TransferPhase::Failed }, Some(msg));
    }

    /// Returns (bytes copied, summary).
    fn copy_job(&self, h: &Handle, job: &BackupJob) -> Result<(u64, String), String> {
        let source = PathBuf::from(&job.source);
        if !source.is_dir() {
            return Err(format!("La carpeta de origen ya no existe: {}", job.source));
        }
        let dest_root = PathBuf::from(&job.dest);
        if !dest_root.starts_with(provider(&self.home, &job.provider_id)?.root) {
            return Err("El destino ya no está dentro de la nube".into());
        }
        fs::create_dir_all(&dest_root).map_err(|e| format!("No se pudo crear {}: {e}", dest_root.display()))?;

        let mut excludes = job.excludes.clone();
        excludes.push(".DS_Store".into());
        let (files, walk_errors) = walk_files(&source, false, &excludes);
        let (target_dir, final_dir) = match job.mode {
            BackupMode::Mirror => (dest_root.clone(), None),
            BackupMode::Snapshot => {
                let name = stamp(now());
                (dest_root.join(format!("{name}{IN_PROGRESS}")), Some(dest_root.join(name)))
            }
        };

        // Decide what to copy (mirror: only new or changed files).
        let todo: Vec<(PathBuf, PathBuf, u64, Option<std::time::SystemTime>)> = files
            .into_iter()
            .filter_map(|f| {
                let rel = f.path.strip_prefix(&source).ok()?.to_path_buf();
                let target = target_dir.join(&rel);
                if job.mode == BackupMode::Mirror {
                    if let Ok(m) = fs::metadata(&target) {
                        let same_time = match (m.modified().ok(), f.mtime) {
                            (Some(a), Some(b)) => a.duration_since(b).or_else(|_| b.duration_since(a)).map(|d| d.as_secs() < 2).unwrap_or(false),
                            _ => false,
                        };
                        if m.len() == f.size && same_time {
                            return None;
                        }
                    }
                }
                Some((f.path, target, f.size, f.mtime))
            })
            .collect();

        let bytes_total: u64 = todo.iter().map(|t| t.2).sum();
        h.update(|t| {
            t.phase = TransferPhase::Copying;
            t.files_total = todo.len();
            t.bytes_total = bytes_total;
        });

        let mut copied = Vec::with_capacity(todo.len());
        let mut done = 0u64;
        let mut errors: Vec<String> = Vec::new();
        let mut error_count = walk_errors;
        for (i, (src, dst, size, mtime)) in todo.iter().enumerate() {
            if h.cancelled() {
                return Err("Cancelado".into());
            }
            match copy_file(src, dst, *mtime, |n| {
                h.progress(done + n, i);
                h.update(|t| t.copy_pct = pct(done + n, bytes_total));
                !h.cancelled()
            }) {
                Ok(()) => copied.push((dst.clone(), *size)),
                Err(e) => {
                    error_count += 1;
                    if errors.len() < 3 {
                        errors.push(format!("{}: {e}", src.display()));
                    }
                }
            }
            done += size;
            h.progress(done, i + 1);
            h.update(|t| t.copy_pct = pct(done, bytes_total));
        }
        h.update(|t| t.copy_pct = 100.0);

        if let (Some(final_dir), BackupMode::Snapshot) = (&final_dir, job.mode) {
            fs::rename(&target_dir, final_dir).map_err(|e| format!("No se pudo cerrar la versión: {e}"))?;
            copied = copied
                .into_iter()
                .filter_map(|(p, s)| Some((final_dir.join(p.strip_prefix(&target_dir).ok()?), s)))
                .collect();
            self.prune(job, &dest_root);
        }

        let copied_bytes: u64 = copied.iter().map(|c| c.1).sum();
        let copied_files = copied.len();
        let uploaded = wait_uploaded(h, copied, UPLOAD_WAIT);
        if h.cancelled() {
            return Err("Cancelado".into());
        }

        let mut msg = if copied_files == 0 {
            "Sin cambios desde el último backup".to_string()
        } else {
            format!("{} copiados ({})", plural(copied_files, "archivo", "archivos"), format_bytes(copied_bytes))
        };
        if !uploaded {
            msg.push_str(". La nube sigue subiendo el resto");
        }
        if error_count > 0 {
            msg.push_str(&format!(". {} con errores", plural(error_count, "archivo", "archivos")));
            if let Some(e) = errors.first() {
                msg.push_str(&format!(": {e}"));
            }
            if copied_files == 0 {
                return Err(msg);
            }
        }
        Ok((copied_bytes, msg))
    }

    /// Deletes snapshot folders beyond `keep_versions` (oldest first).
    fn prune(&self, job: &BackupJob, dest_root: &Path) {
        let mut versions: Vec<(u64, PathBuf)> = fs::read_dir(dest_root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let ts = parse_stamp(&name)?;
                (e.path().parent() == Some(dest_root) && e.path().is_dir()).then(|| (ts, e.path()))
            })
            .collect();
        versions.sort_by_key(|v| std::cmp::Reverse(v.0));
        for (_, p) in versions.into_iter().skip(job.keep_versions.max(1) as usize) {
            match fs::remove_dir_all(&p) {
                Ok(()) => self.log(&format!("[{}] versión antigua eliminada: {}", job.name, p.display())),
                Err(e) => self.log(&format!("[{}] no se pudo eliminar {}: {e}", job.name, p.display())),
            }
        }
    }

    pub fn versions(&self, id: &str) -> Result<Vec<BackupVersion>, String> {
        let job = self.get(id)?;
        let dest = PathBuf::from(&job.dest);
        if !dest.is_dir() {
            return Ok(vec![]);
        }
        let version = |path: PathBuf, label: String, created_at: Option<u64>| {
            let a = scan_tree(&path, false, false);
            BackupVersion {
                path: path.display().to_string(),
                label,
                created_at,
                size_bytes: a.total,
                file_count: a.files,
                cloud_only_bytes: a.cloud_only_bytes,
            }
        };
        let mut snaps: Vec<(u64, PathBuf, String)> = fs::read_dir(&dest)
            .map_err(|e| e.to_string())?
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                Some((parse_stamp(&name)?, e.path(), name))
            })
            .filter(|(_, p, _)| p.is_dir())
            .collect();
        snaps.sort_by_key(|v| std::cmp::Reverse(v.0));
        if job.mode == BackupMode::Mirror || snaps.is_empty() {
            let mut v = vec![version(dest.clone(), "Copia espejo (última)".into(), job.last_run)];
            // A job switched from snapshot to mirror still lists its old versions.
            v.extend(snaps.into_iter().map(|(ts, p, name)| version(p, name, Some(ts))));
            return Ok(v);
        }
        Ok(snaps.into_iter().map(|(ts, p, name)| version(p, name, Some(ts))).collect())
    }

    /// Copies one version back to a new folder (default ~/Downloads). Cloud-only
    /// files are downloaded on the way.
    pub fn restore(self: &Arc<Self>, id: &str, version_path: &str, target: Option<String>) -> Result<Transfer, String> {
        let job = self.get(id)?;
        let version = PathBuf::from(version_path);
        if version.components().any(|c| matches!(c, std::path::Component::ParentDir))
            || !version.starts_with(&job.dest)
            || !version.is_dir()
        {
            return Err("Esa versión no pertenece a este backup".into());
        }
        let parent = target.filter(|t| !t.trim().is_empty()).map(PathBuf::from).unwrap_or_else(|| self.home.join("Downloads"));
        if !parent.is_absolute() || !parent.is_dir() {
            return Err(format!("No existe la carpeta {}", parent.display()));
        }
        let base = format!("{} (restaurado {})", sanitize(&job.name), stamp(now()));
        let mut out = parent.join(&base);
        let mut n = 2;
        while out.exists() {
            out = parent.join(format!("{base} {n}"));
            n += 1;
        }
        let h = self.transfers.start(
            TransferKind::Restore,
            format!("Restaurar «{}»", job.name),
            job.provider_id.clone(),
            version.display().to_string(),
            out.display().to_string(),
            None,
        );
        let worker = h.clone();
        std::thread::spawn(move || run_restore(worker, version, out));
        h.get().ok_or_else(|| "No se pudo iniciar la restauración".into())
    }
}

fn run_restore(h: Handle, version: PathBuf, out: PathBuf) {
    let (files, _) = walk_files(&version, true, &[]);
    let total: u64 = files.iter().map(|f| f.size).sum();
    let local: u64 = files.iter().filter(|f| !f.dataless).map(|f| f.size).sum();
    h.update(|t| {
        t.phase = TransferPhase::Copying;
        t.files_total = files.len();
        t.bytes_total = total;
        t.download_pct = pct(local, total);
    });
    if let Err(e) = fs::create_dir_all(&out) {
        return h.finish(TransferPhase::Failed, Some(format!("No se pudo crear {}: {e}", out.display())));
    }
    let mut done = 0u64;
    let mut downloaded = local;
    let mut errors = 0usize;
    let mut first_err = None;
    for (i, f) in files.iter().enumerate() {
        if h.cancelled() {
            return h.finish(TransferPhase::Cancelled, Some(format!("Cancelada. Lo copiado quedó en {}", out.display())));
        }
        let Ok(rel) = f.path.strip_prefix(&version) else { continue };
        let dst = out.join(rel);
        if f.dataless {
            h.update(|t| t.phase = TransferPhase::Downloading);
        }
        // Reading a cloud-only file makes the provider download it first.
        let r = copy_file(&f.path, &dst, f.mtime, |n| {
            h.progress(done + n, i);
            h.update(|t| t.copy_pct = pct(done + n, total));
            !h.cancelled()
        });
        if let Err(e) = r {
            errors += 1;
            first_err.get_or_insert(format!("{}: {e}", f.path.display()));
        }
        done += f.size;
        if f.dataless {
            downloaded += f.size;
        }
        h.progress(done, i + 1);
        h.update(|t| {
            t.phase = TransferPhase::Copying;
            t.copy_pct = pct(done, total);
            t.download_pct = pct(downloaded, total);
        });
    }
    let msg = match first_err {
        None => format!("Restaurado en {}", out.display()),
        Some(e) => format!("Restaurado en {} · {} con errores: {e}", out.display(), plural(errors, "archivo", "archivos")),
    };
    h.finish(if errors == 0 || errors < files.len() { TransferPhase::Done } else { TransferPhase::Failed }, Some(msg));
}

fn pct(done: u64, total: u64) -> f32 {
    if total == 0 {
        100.0
    } else {
        (done as f64 / total as f64 * 100.0).min(100.0) as f32
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Chunked copy with progress callback (bytes of this file). The callback
/// returns false to abort. Never overwrites a newer file in a restore: the
/// target folder is always new.
fn copy_file(src: &Path, dst: &Path, mtime: Option<std::time::SystemTime>, mut progress: impl FnMut(u64) -> bool) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut input = fs::File::open(src).map_err(|e| e.to_string())?;
    let tmp = dst.with_file_name(format!(".{}.iclear-tmp", dst.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    let mut output = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; CHUNK];
    let mut n_total = 0u64;
    let result = (|| {
        loop {
            let n = input.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            n_total += n as u64;
            if !progress(n_total) {
                return Err("Cancelado".to_string());
            }
        }
        if let Some(t) = mtime {
            let _ = output.set_modified(t);
        }
        output.sync_all().map_err(|e| e.to_string())
    })();
    drop(output);
    match result {
        Ok(()) => fs::rename(&tmp, dst).map_err(|e| e.to_string()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Arc<BackupService>, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join("Library/CloudStorage/Dropbox")).unwrap();
        let src = home.path().join("Documents/proyecto");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::create_dir_all(src.join("node_modules/x")).unwrap();
        fs::write(src.join("a.txt"), b"hola").unwrap();
        fs::write(src.join("sub/b.bin"), vec![7u8; 3 * CHUNK + 5]).unwrap();
        fs::write(src.join("node_modules/x/skip.js"), b"no").unwrap();
        let svc = BackupService::new(home.path().to_path_buf(), Transfers::new());
        (home, svc, src)
    }

    fn draft(src: &Path, mode: BackupMode) -> BackupDraft {
        BackupDraft {
            id: None,
            name: "Proyecto".into(),
            source: src.display().to_string(),
            provider_id: "Dropbox".into(),
            dest_folder: None,
            mode,
            schedule: BackupSchedule::Manual,
            keep_versions: 2,
            excludes: vec!["node_modules".into()],
            enabled: true,
        }
    }

    fn wait(svc: &BackupService, id: u64) -> Transfer {
        for _ in 0..200 {
            let t = svc.transfers.list().into_iter().find(|t| t.id == id).unwrap();
            if t.phase.is_finished() {
                return t;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("timeout");
    }

    #[test]
    fn stamps_round_trip() {
        let t = 1_763_200_000;
        assert_eq!(parse_stamp(&stamp(t)), Some(t));
        assert_eq!(parse_stamp("Proyecto"), None);
        assert_eq!(parse_stamp("2026-11-15 16.30.05 (en curso)"), None);
    }

    #[test]
    fn mirror_copies_only_changes_and_respects_excludes() {
        let (home, svc, src) = setup();
        let job = svc.save_job(draft(&src, BackupMode::Mirror)).unwrap();
        assert!(job.dest.ends_with("Dropbox/iclear Backups/Proyecto"));

        let t = wait(&svc, svc.run(&job.id).unwrap().id);
        assert_eq!(t.phase, TransferPhase::Done, "{:?}", t.message);
        assert_eq!(t.files_total, 2);
        assert_eq!(t.copy_pct, 100.0);
        let dest = PathBuf::from(&job.dest);
        assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"hola");
        assert_eq!(fs::metadata(dest.join("sub/b.bin")).unwrap().len(), 3 * CHUNK as u64 + 5);
        assert!(!dest.join("node_modules").exists());

        // Second run: nothing changed.
        let t = wait(&svc, svc.run(&job.id).unwrap().id);
        assert_eq!(t.files_total, 0);
        assert_eq!(svc.list()[0].last_message.as_deref(), Some("Sin cambios desde el último backup"));
        drop(home);
    }

    #[test]
    fn snapshots_keep_n_versions_and_restore_to_new_folder() {
        let (home, svc, src) = setup();
        let job = svc.save_job(draft(&src, BackupMode::Snapshot)).unwrap();
        for _ in 0..3 {
            wait(&svc, svc.run(&job.id).unwrap().id);
            std::thread::sleep(Duration::from_millis(1100)); // distinct stamps
        }
        let versions = svc.versions(&job.id).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].file_count, 2);

        let target = home.path().join("restore");
        fs::create_dir(&target).unwrap();
        let t = svc.restore(&job.id, &versions[0].path, Some(target.display().to_string())).unwrap();
        let t = wait(&svc, t.id);
        assert_eq!(t.phase, TransferPhase::Done, "{:?}", t.message);
        let out = PathBuf::from(&t.dest);
        assert!(out.starts_with(&target));
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"hola");

        assert!(svc.restore(&job.id, "/etc", None).is_err());
    }

    #[test]
    fn rejects_dangerous_sources() {
        let (home, svc, _) = setup();
        let mut d = draft(home.path(), BackupMode::Mirror);
        assert!(svc.save_job(d.clone()).is_err(), "home itself");
        d.source = home.path().join("Library").display().to_string();
        assert!(svc.save_job(d.clone()).is_err(), "contains the clouds");
        d.source = "/".into();
        assert!(svc.save_job(d).is_err());
    }
}
