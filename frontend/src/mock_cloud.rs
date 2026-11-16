//! Mock cloud drives, transfers and backups for the browser preview.
//! Transfers advance with wall-clock time so progress bars move.

use std::cell::RefCell;

use iclear_protocol::*;

use crate::api::sleep;

const HOME: &str = "/Users/usuario";
const MB: u64 = 1_000_000;
const GB: u64 = 1_000_000_000;

struct MockTransfer {
    t: Transfer,
    started_ms: f64,
    /// Seconds for the copy/download phase and for the upload phase.
    secs: (f64, f64),
    cancelled: bool,
}

struct Sim {
    transfers: Vec<MockTransfer>,
    backups: Vec<BackupJob>,
    evicted: Vec<String>,
    downloaded: Vec<String>,
    next_id: u64,
}

thread_local! {
    static SIM: RefCell<Sim> = RefCell::new(Sim {
        transfers: Vec::new(),
        backups: initial_backups(),
        evicted: Vec::new(),
        downloaded: Vec::new(),
        next_id: 1,
    });
}

fn now_s() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

pub fn providers() -> Vec<CloudProvider> {
    let p = |id: &str, kind, name: &str, account: Option<&str>, root: String, writable: String| CloudProvider {
        id: id.into(),
        kind,
        name: name.into(),
        account: account.map(Into::into),
        root,
        writable_root: Some(writable),
        accessible: true,
    };
    let cs = format!("{HOME}/Library/CloudStorage");
    vec![
        p(
            "icloud",
            CloudKind::ICloud,
            "iCloud Drive",
            None,
            format!("{HOME}/Library/Mobile Documents/com~apple~CloudDocs"),
            format!("{HOME}/Library/Mobile Documents/com~apple~CloudDocs"),
        ),
        p(
            "GoogleDrive-ana@estudio.cl",
            CloudKind::GoogleDrive,
            "Google Drive",
            Some("ana@estudio.cl"),
            format!("{cs}/GoogleDrive-ana@estudio.cl"),
            format!("{cs}/GoogleDrive-ana@estudio.cl/Mi unidad"),
        ),
        p("Dropbox", CloudKind::Dropbox, "Dropbox", None, format!("{cs}/Dropbox"), format!("{cs}/Dropbox")),
        p(
            "OneDrive-Personal",
            CloudKind::OneDrive,
            "OneDrive",
            Some("Personal"),
            format!("{cs}/OneDrive-Personal"),
            format!("{cs}/OneDrive-Personal"),
        ),
    ]
}

fn root_of(id: &str) -> Option<String> {
    providers().into_iter().find(|p| p.id == id).map(|p| p.root)
}

/// (total, downloaded, files, cloud_only, pending, pending_bytes)
fn base_stats(id: &str) -> (u64, u64, usize, usize, usize, u64) {
    match id {
        "icloud" => (176 * GB, 12 * GB, 84_210, 71_480, 3, 48 * MB),
        "Dropbox" => (38 * GB, 38 * GB, 12_904, 0, 0, 0),
        "OneDrive-Personal" => (4 * GB, 1 * GB, 2_310, 1_870, 0, 0),
        _ => (178 * GB, 2 * GB, 1_118_179, 1_101_002, 12, 310 * MB),
    }
}

pub async fn stats(id: &str, _rescan: bool) -> Result<CloudStats, String> {
    sleep(350).await;
    let (total, mut downloaded, files, mut cloud_only, mut pending, mut pending_bytes) = base_stats(id);
    SIM.with(|s| {
        let s = s.borrow();
        let root = root_of(id).unwrap_or_default();
        for p in &s.downloaded {
            if p.starts_with(&root) {
                downloaded = (downloaded + 2 * GB).min(total);
                cloud_only = cloud_only.saturating_sub(800);
            }
        }
        for p in &s.evicted {
            if p.starts_with(&root) {
                downloaded = downloaded.saturating_sub(GB);
                cloud_only += 400;
            }
        }
        // Running backups into this drive show up as pending uploads.
        for t in &s.transfers {
            if t.t.provider_id == id && !t.t.phase.is_finished() && t.t.kind == TransferKind::Backup {
                pending += t.t.files_total.saturating_sub((t.t.upload_pct as usize * t.t.files_total) / 100);
                pending_bytes += (t.t.bytes_total as f64 * (1.0 - t.t.upload_pct as f64 / 100.0)) as u64;
            }
        }
    });
    let pct = |a: u64, b: u64| if b == 0 { 100.0 } else { (a as f64 / b as f64 * 100.0) as f32 };
    Ok(CloudStats {
        provider_id: id.into(),
        total_bytes: total,
        downloaded_bytes: downloaded,
        on_disk_bytes: downloaded,
        file_count: files,
        folder_count: files / 9,
        cloud_only_count: cloud_only,
        cloud_only_bytes: total - downloaded,
        pending_upload_count: pending,
        pending_upload_bytes: pending_bytes,
        uploading_count: pending.min(2),
        downloading_count: 0,
        conflict_count: if id == "Dropbox" { 1 } else { 0 },
        download_pct: pct(downloaded, total),
        upload_pct: pct(total - pending_bytes.min(total), total),
        unlisted_folders: if id == "icloud" { 76 } else { 0 },
        skipped_errors: 0,
        scanning: false,
        complete: true,
        status_window_days: 14,
        scanned_at: now_s(),
        scan_ms: 11_400,
    })
}

fn item(dir: &str, name: &str, is_dir: bool, size: u64, dl: f32, up: f32, state: CloudItemState) -> CloudItem {
    CloudItem {
        name: name.into(),
        path: format!("{dir}/{name}"),
        is_dir,
        size_bytes: size,
        downloaded_bytes: (size as f64 * dl as f64 / 100.0) as u64,
        file_count: if is_dir { (size / (4 * MB)).max(1) as usize } else { 1 },
        state,
        download_pct: dl,
        upload_pct: up,
        modified: Some(now_s() - (size % 90) * 86_400),
        partial: false,
    }
}

pub async fn list(id: &str, path: Option<&str>) -> Result<CloudListing, String> {
    sleep(300).await;
    let root = root_of(id).ok_or("No se encontró la nube")?;
    let dir = path.map(str::to_string).unwrap_or_else(|| root.clone());
    use CloudItemState::*;
    let at_root = dir == root;
    let mut items = if at_root && id.starts_with("GoogleDrive") {
        vec![
            item(&dir, "Mi unidad", true, 125 * GB, 1.6, 99.8, PendingUpload),
            item(&dir, "Otros ordenadores", true, 32 * GB, 0.0, 100.0, CloudOnly),
            item(&dir, "Unidades compartidas", true, 21 * GB, 0.0, 100.0, CloudOnly),
        ]
    } else if dir.ends_with("iclear Backups") {
        vec![item(&dir, "Documentos", true, 3 * GB, 100.0, 97.5, Uploading)]
    } else {
        vec![
            item(&dir, "Proyectos", true, 48 * GB, 22.0, 100.0, Partial),
            item(&dir, "Fotos 2025", true, 31 * GB, 0.0, 100.0, CloudOnly),
            item(&dir, "iclear Backups", true, 3 * GB, 100.0, 97.5, Uploading),
            item(&dir, "Clientes", true, 2 * GB, 100.0, 100.0, Synced),
            item(&dir, "Presentación final.key", false, 412 * MB, 100.0, 0.0, PendingUpload),
            item(&dir, "Contrato firmado.pdf", false, 2 * MB, 100.0, 100.0, Synced),
            item(&dir, "Video demo.mov", false, 1_800 * MB, 0.0, 100.0, CloudOnly),
            item(&dir, "Presupuesto (conflicto).numbers", false, 840_000, 100.0, 100.0, Conflict),
        ]
    };
    SIM.with(|s| {
        let s = s.borrow();
        for it in &mut items {
            if s.downloaded.iter().any(|p| it.path.starts_with(p.as_str())) {
                it.download_pct = 100.0;
                it.downloaded_bytes = it.size_bytes;
                if matches!(it.state, CloudOnly | Partial) {
                    it.state = Synced;
                }
            }
            if s.evicted.iter().any(|p| it.path.starts_with(p.as_str())) && it.upload_pct >= 100.0 {
                it.download_pct = 0.0;
                it.downloaded_bytes = 0;
                it.state = CloudOnly;
            }
        }
    });
    let parent = (!at_root).then(|| dir.rsplit_once('/').map(|(p, _)| p.to_string())).flatten();
    Ok(CloudListing { provider_id: id.into(), path: dir, parent, items, skipped_errors: 0 })
}

pub async fn free_up(path: &str) -> Result<CloudActionReport, String> {
    sleep(700).await;
    SIM.with(|s| s.borrow_mut().evicted.push(path.to_string()));
    Ok(CloudActionReport { files: 214, bytes: 3_400 * MB, skipped_not_uploaded: 2, errors: vec![] })
}

fn new_transfer(kind: TransferKind, label: String, provider_id: &str, source: &str, dest: &str, backup_id: Option<String>, bytes: u64, files: usize, secs: (f64, f64)) -> Transfer {
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        let id = s.next_id;
        s.next_id += 1;
        let t = Transfer {
            id,
            kind,
            phase: TransferPhase::Preparing,
            label,
            provider_id: provider_id.into(),
            backup_id,
            source: source.into(),
            dest: dest.into(),
            files_total: files,
            files_done: 0,
            bytes_total: bytes,
            bytes_done: 0,
            copy_pct: 0.0,
            upload_pct: 0.0,
            download_pct: 0.0,
            bytes_per_sec: 0,
            started_at: now_s(),
            finished_at: None,
            message: None,
        };
        s.transfers.insert(0, MockTransfer { t: t.clone(), started_ms: now_ms(), secs, cancelled: false });
        t
    })
}

fn provider_of(path: &str) -> Option<CloudProvider> {
    providers().into_iter().find(|p| path.starts_with(&p.root))
}

pub fn download(path: &str) -> Result<Transfer, String> {
    let prov = provider_of(path).ok_or("La ruta no está dentro de una nube sincronizada")?;
    let name = path.rsplit('/').next().unwrap_or("archivo");
    SIM.with(|s| s.borrow_mut().downloaded.push(path.to_string()));
    Ok(new_transfer(TransferKind::Download, format!("Descargar {name}"), &prov.id, path, path, None, 6 * GB, 1_420, (24.0, 0.0)))
}

fn advance(m: &mut MockTransfer) {
    if m.t.phase.is_finished() {
        return;
    }
    if m.cancelled {
        m.t.phase = TransferPhase::Cancelled;
        m.t.finished_at = Some(now_s());
        m.t.bytes_per_sec = 0;
        m.t.message = Some("Cancelada".into());
        return;
    }
    let el = (now_ms() - m.started_ms) / 1000.0 - 0.8;
    if el < 0.0 {
        return;
    }
    let (a, b) = m.secs;
    let f1 = (el / a).min(1.0);
    let ease = |x: f64| 1.0 - (1.0 - x).powf(1.4);
    let t = &mut m.t;
    t.bytes_done = (t.bytes_total as f64 * ease(f1)) as u64;
    t.files_done = (t.files_total as f64 * ease(f1)) as usize;
    t.bytes_per_sec = if f1 < 1.0 { (t.bytes_total as f64 / a * (1.2 - f1 * 0.4)) as u64 } else { 0 };
    match t.kind {
        TransferKind::Download => {
            t.phase = TransferPhase::Downloading;
            t.download_pct = (ease(f1) * 100.0) as f32;
            if f1 >= 1.0 {
                t.phase = TransferPhase::Done;
                t.finished_at = Some(now_s());
            }
        }
        TransferKind::Restore => {
            t.phase = if f1 < 0.4 { TransferPhase::Downloading } else { TransferPhase::Copying };
            t.download_pct = (ease((f1 * 1.6).min(1.0)) * 100.0) as f32;
            t.copy_pct = (ease(f1) * 100.0) as f32;
            if f1 >= 1.0 {
                t.phase = TransferPhase::Done;
                t.finished_at = Some(now_s());
                t.message = Some(format!("Restaurado en {}", t.dest));
            }
        }
        TransferKind::Backup => {
            t.phase = TransferPhase::Copying;
            t.copy_pct = (ease(f1) * 100.0) as f32;
            // The provider starts uploading while iclear is still copying.
            let f2 = ((el - a * 0.3) / (a * 0.7 + b)).clamp(0.0, 1.0);
            t.upload_pct = (f2 * 100.0) as f32;
            if f1 >= 1.0 {
                t.phase = TransferPhase::Uploading;
                t.bytes_per_sec = (t.bytes_total as f64 / b * 0.8) as u64;
            }
            if f2 >= 1.0 {
                t.phase = TransferPhase::Done;
                t.finished_at = Some(now_s());
                t.bytes_per_sec = 0;
                t.message = Some(format!("{} archivos copiados ({})", t.files_total, format_bytes(t.bytes_total)));
            }
        }
    }
}

pub fn transfers() -> Vec<Transfer> {
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        let mut finished_backups = Vec::new();
        for m in &mut s.transfers {
            let was = m.t.phase.is_finished();
            advance(m);
            if !was && m.t.phase.is_finished() {
                if let Some(id) = &m.t.backup_id {
                    finished_backups.push((id.clone(), m.t.phase == TransferPhase::Done, m.t.message.clone(), m.t.bytes_total));
                }
            }
        }
        for (id, ok, msg, bytes) in finished_backups {
            if let Some(j) = s.backups.iter_mut().find(|j| j.id == id) {
                j.last_run = Some(now_s());
                j.last_ok = Some(ok);
                j.last_message = msg.or(Some("Cancelado".into()));
                j.last_bytes = bytes;
                j.next_run = j.schedule.interval_secs().map(|i| now_s() + i);
            }
        }
        s.transfers.iter().map(|m| m.t.clone()).collect()
    })
}

pub fn cancel(id: u64) -> Result<(), String> {
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        let m = s.transfers.iter_mut().find(|m| m.t.id == id).ok_or("Transferencia no encontrada")?;
        m.cancelled = true;
        Ok(())
    })
}

pub fn clear_transfers() {
    SIM.with(|s| s.borrow_mut().transfers.retain(|m| !m.t.phase.is_finished()));
}

fn initial_backups() -> Vec<BackupJob> {
    let gd = format!("{HOME}/Library/CloudStorage/GoogleDrive-ana@estudio.cl/Mi unidad/iclear Backups");
    let ic = format!("{HOME}/Library/Mobile Documents/com~apple~CloudDocs/iclear Backups");
    let now = (js_sys::Date::now() / 1000.0) as u64;
    vec![
        BackupJob {
            id: "bk1".into(),
            name: "Documentos".into(),
            source: format!("{HOME}/Documents"),
            provider_id: "GoogleDrive-ana@estudio.cl".into(),
            dest: format!("{gd}/Documentos"),
            mode: BackupMode::Snapshot,
            schedule: BackupSchedule::Daily,
            keep_versions: 7,
            excludes: vec![".git".into(), "node_modules".into()],
            enabled: true,
            created_at: now - 40 * 86_400,
            last_run: Some(now - 5 * 3600),
            last_ok: Some(true),
            last_message: Some("1.284 archivos copiados (3,1 GB)".into()),
            last_bytes: 3_100 * MB,
            next_run: Some(now + 19 * 3600),
        },
        BackupJob {
            id: "bk2".into(),
            name: "Proyectos".into(),
            source: format!("{HOME}/Developer"),
            provider_id: "icloud".into(),
            dest: format!("{ic}/Proyectos"),
            mode: BackupMode::Mirror,
            schedule: BackupSchedule::Hourly,
            keep_versions: 1,
            excludes: vec![".git".into(), "node_modules".into(), "target".into()],
            enabled: true,
            created_at: now - 12 * 86_400,
            last_run: Some(now - 50 * 60),
            last_ok: Some(false),
            last_message: Some("12 archivos copiados (40 MB). 2 archivos con errores: permiso denegado".into()),
            last_bytes: 40 * MB,
            next_run: Some(now + 10 * 60),
        },
    ]
}

pub fn backups() -> Vec<BackupJob> {
    SIM.with(|s| s.borrow().backups.clone())
}

pub fn save_backup(d: BackupDraft) -> Result<BackupJob, String> {
    if d.name.trim().is_empty() {
        return Err("Ponle un nombre al backup".into());
    }
    if !d.source.starts_with('/') && !d.source.starts_with('~') {
        return Err("La carpeta de origen debe ser una ruta completa".into());
    }
    if d.source.trim_end_matches('/') == HOME || d.source.trim() == "~" {
        return Err("Elige una carpeta concreta (Documentos, Escritorio…), no toda tu carpeta de usuario".into());
    }
    let prov = providers().into_iter().find(|p| p.id == d.provider_id).ok_or("Elige una nube de destino")?;
    let folder = d.dest_folder.clone().filter(|f| !f.trim().is_empty()).unwrap_or_else(|| "iclear Backups".into());
    let now = now_s();
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        let id = d.id.clone().unwrap_or_else(|| format!("bk{}", now));
        let prev = s.backups.iter().find(|j| j.id == id).cloned();
        let job = BackupJob {
            id: id.clone(),
            name: d.name.trim().into(),
            source: d.source.replacen('~', HOME, 1),
            provider_id: prov.id.clone(),
            dest: format!("{}/{}/{}", prov.writable_root.unwrap_or(prov.root), folder, d.name.trim()),
            mode: d.mode,
            schedule: d.schedule,
            keep_versions: d.keep_versions.clamp(1, 100),
            excludes: d.excludes.clone(),
            enabled: d.enabled,
            created_at: prev.as_ref().map(|p| p.created_at).unwrap_or(now),
            last_run: prev.as_ref().and_then(|p| p.last_run),
            last_ok: prev.as_ref().and_then(|p| p.last_ok),
            last_message: prev.as_ref().and_then(|p| p.last_message.clone()),
            last_bytes: prev.as_ref().map(|p| p.last_bytes).unwrap_or(0),
            next_run: if d.enabled { d.schedule.interval_secs().map(|_| now + 60) } else { None },
        };
        match s.backups.iter_mut().find(|j| j.id == id) {
            Some(slot) => *slot = job.clone(),
            None => s.backups.push(job.clone()),
        }
        Ok(job)
    })
}

pub fn delete_backup(id: &str) -> Result<(), String> {
    SIM.with(|s| {
        let mut s = s.borrow_mut();
        if s.transfers.iter().any(|m| m.t.backup_id.as_deref() == Some(id) && !m.t.phase.is_finished()) {
            return Err("El backup se está ejecutando; cancélalo primero".into());
        }
        s.backups.retain(|j| j.id != id);
        Ok(())
    })
}

pub fn run_backup(id: &str) -> Result<Transfer, String> {
    let job = backups().into_iter().find(|j| j.id == id).ok_or("Backup no encontrado")?;
    let running = SIM.with(|s| s.borrow().transfers.iter().any(|m| m.t.backup_id.as_deref() == Some(id) && !m.t.phase.is_finished()));
    if running {
        return Err("Este backup ya se está ejecutando".into());
    }
    let (bytes, files) = if job.mode == BackupMode::Mirror { (180 * MB, 46) } else { (3_100 * MB, 1_284) };
    Ok(new_transfer(
        TransferKind::Backup,
        format!("Backup «{}»", job.name),
        &job.provider_id,
        &job.source,
        &job.dest,
        Some(job.id.clone()),
        bytes,
        files,
        (14.0, 10.0),
    ))
}

pub async fn versions(id: &str) -> Result<Vec<BackupVersion>, String> {
    sleep(300).await;
    let job = backups().into_iter().find(|j| j.id == id).ok_or("Backup no encontrado")?;
    if job.mode == BackupMode::Mirror {
        return Ok(vec![BackupVersion {
            path: job.dest.clone(),
            label: "Copia espejo (última)".into(),
            created_at: job.last_run,
            size_bytes: 2_400 * MB,
            file_count: 8_210,
            cloud_only_bytes: 1_900 * MB,
        }]);
    }
    let now = now_s();
    Ok((0..job.keep_versions.min(5) as u64)
        .map(|i| {
            let ts = now - 5 * 3600 - i * 86_400;
            BackupVersion {
                path: format!("{}/v{i}", job.dest),
                label: format!("Versión de hace {} días", i + 1).replace("hace 1 días", "ayer"),
                created_at: Some(ts),
                size_bytes: 3_100 * MB - i * 40 * MB,
                file_count: 1_284 - i as usize * 9,
                cloud_only_bytes: if i == 0 { 0 } else { 3_000 * MB },
            }
        })
        .collect())
}

pub fn restore(id: &str, version_path: &str) -> Result<Transfer, String> {
    let job = backups().into_iter().find(|j| j.id == id).ok_or("Backup no encontrado")?;
    Ok(new_transfer(
        TransferKind::Restore,
        format!("Restaurar «{}»", job.name),
        &job.provider_id,
        version_path,
        &format!("{HOME}/Downloads/{} (restaurado)", job.name),
        None,
        3_100 * MB,
        1_284,
        (16.0, 0.0),
    ))
}
