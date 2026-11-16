//! Installed apps: how much each one weighs (bundle + data it left in
//! ~/Library), when it was last used, and what it leaves behind after being
//! removed.
//!
//! "Last used" combines Spotlight's `kMDItemLastUsedDate` (updated by
//! LaunchServices every time an app is opened) with iclear's own record of
//! which app was in front (`MemoryIntel::focus_map`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use iclear_protocol::{AppLeftover, AppsReport, InstalledApp, OrphanGroup, PreviewItem, UsageVerdict};
use rayon::prelude::*;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::memory::{outermost_app, read_bundle, BundleMeta};
use crate::storage::scan::{days_since, path_size};

/// Folders in ~/Library whose direct children belong to one app each.
/// (relative path, label shown in the UI)
pub const APP_DATA_DIRS: &[(&str, &str)] = &[
    ("Library/Application Support", "Soporte"),
    ("Library/Caches", "Caché"),
    ("Library/Containers", "Contenedor"),
    ("Library/Preferences", "Preferencias"),
    ("Library/Saved Application State", "Estado guardado"),
    ("Library/HTTPStorages", "Datos web"),
    ("Library/WebKit", "Datos web"),
    ("Library/Application Scripts", "Scripts"),
    ("Library/Logs", "Registros"),
    ("Library/LaunchAgents", "Agente de inicio"),
];

const ACTIVE_DAYS: u64 = 30;
const OCCASIONAL_DAYS: u64 = 90;
const ORPHAN_MIN_AGE_DAYS: u64 = 30;

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Parses Spotlight dates like `2026-10-13 04:38:32 +0000` into unix seconds.
pub fn parse_md_date(s: &str) -> Option<u64> {
    let mut it = s.split_whitespace();
    let date = it.next()?;
    let time = it.next()?;
    let tz = it.next().unwrap_or("+0000");
    let d: Vec<i64> = date.split('-').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let t: Vec<i64> = time.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    if d.len() != 3 || t.len() != 3 {
        return None;
    }
    // Days from civil (Howard Hinnant).
    let (y, m, day) = (if d[1] <= 2 { d[0] - 1 } else { d[0] }, d[1], d[2]);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let tzv: i64 = tz.trim_start_matches(['+', '-']).parse().ok()?;
    let offset = sign * ((tzv / 100) * 3600 + (tzv % 100) * 60);
    let secs = days * 86400 + t[0] * 3600 + t[1] * 60 + t[2] - offset;
    (secs >= 0).then_some(secs as u64)
}

struct Spotlight {
    added: Option<u64>,
    last_used: Option<u64>,
    use_count: Option<u64>,
}

fn spotlight(path: &Path) -> Spotlight {
    // With -raw, values come NUL-separated in alphabetical attribute order.
    let out = Command::new("/usr/bin/mdls")
        .args(["-name", "kMDItemDateAdded", "-name", "kMDItemLastUsedDate", "-name", "kMDItemUseCount", "-raw", "-nullMarker", "NULL"])
        .arg(path)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let v: Vec<&str> = out.split('\0').collect();
    let get = |i: usize| v.get(i).copied().filter(|s| *s != "NULL" && !s.is_empty());
    Spotlight {
        added: get(0).and_then(parse_md_date),
        last_used: get(1).and_then(parse_md_date),
        use_count: get(2).and_then(|s| s.trim().parse().ok()),
    }
}

fn app_bundles(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        if p.extension().map(|x| x == "app").unwrap_or(false) {
            out.push(p);
        } else if let Ok(rd2) = std::fs::read_dir(&p) {
            // Vendor folders: /Applications/Utilities, /Applications/Adobe X/...
            out.extend(
                rd2.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().map(|x| x == "app").unwrap_or(false) && p.is_dir()),
            );
        }
    }
    out
}

fn writable(p: &Path) -> bool {
    let Ok(c) = std::ffi::CString::new(p.as_os_str().to_string_lossy().as_bytes()) else { return false };
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// Every direct child of `APP_DATA_DIRS`: (path, label, lowercase stem).
pub struct DataIndex(Vec<(PathBuf, &'static str, String)>);

impl DataIndex {
    pub fn build(home: &Path) -> Self {
        let mut v = Vec::new();
        for (dir, label) in APP_DATA_DIRS {
            let Ok(rd) = std::fs::read_dir(home.join(dir)) else { continue };
            for e in rd.flatten() {
                let fname = e.file_name().to_string_lossy().to_string();
                v.push((e.path(), *label, strip_known_ext(&fname).to_lowercase()));
            }
        }
        Self(v)
    }
}

/// Keys an app's data folders may be named after. `None` for the id = matched by name.
fn name_keys(path: &Path, meta: &BundleMeta) -> Vec<String> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string());
    [Some(meta.name.clone()), stem]
        .into_iter()
        .flatten()
        .map(|n| n.to_lowercase())
        // Too generic to match folders by name safely.
        .filter(|n| n.len() >= 4 && !["electron", "launcher", "helper", "update", "updater", "uninstall", "installer"].contains(&n.as_str()))
        .collect()
}

/// Leftovers of each app (same order as `apps`). An entry matched by bundle id
/// goes only to the app(s) with that id; an entry matched only by name goes
/// to an app only if no other app also claims it. Ambiguous entries are
/// never offered for deletion.
pub fn assign_leftovers(index: &DataIndex, apps: &[(PathBuf, BundleMeta)]) -> Vec<Vec<AppLeftover>> {
    let ids: Vec<Option<String>> = apps.iter().map(|(_, m)| m.id.as_ref().map(|i| i.to_lowercase())).collect();
    let names: Vec<Vec<String>> = apps.iter().map(|(p, m)| name_keys(p, m)).collect();
    let mut claims: Vec<(usize, Vec<usize>)> = Vec::new();
    for (e, (_, _, stem)) in index.0.iter().enumerate() {
        let by_id: Vec<usize> = (0..apps.len())
            .filter(|&k| ids[k].as_deref().map(|id| stem == id || stem.starts_with(&format!("{id}."))).unwrap_or(false))
            .collect();
        let owners = if !by_id.is_empty() {
            by_id
        } else {
            let by_name: Vec<usize> = (0..apps.len()).filter(|&k| names[k].iter().any(|n| n == stem)).collect();
            if by_name.len() == 1 { by_name } else { vec![] }
        };
        if !owners.is_empty() {
            claims.push((e, owners));
        }
    }
    let sized: Vec<(usize, u64)> = claims.par_iter().map(|(e, _)| (*e, path_size(&index.0[*e].0).0)).collect();
    let mut out: Vec<Vec<AppLeftover>> = vec![Vec::new(); apps.len()];
    for ((e, owners), (_, size)) in claims.iter().zip(sized) {
        let (path, label, _) = &index.0[*e];
        for &k in owners {
            out[k].push(AppLeftover { path: path.to_string_lossy().to_string(), kind: label.to_string(), size_bytes: size });
        }
    }
    out
}

fn installed_bundles(home: &Path) -> Vec<(PathBuf, BundleMeta)> {
    let mut bundles = app_bundles(Path::new("/Applications"));
    bundles.extend(app_bundles(&home.join("Applications")));
    bundles.into_par_iter().filter_map(|p| read_bundle(&p).map(|m| (p, m))).collect()
}

fn strip_known_ext(name: &str) -> &str {
    for ext in [".plist", ".savedState", ".binarycookies"] {
        if let Some(s) = name.strip_suffix(ext) {
            return s;
        }
    }
    name
}

fn ago(d: u64) -> String {
    match d {
        0 => "hoy".to_string(),
        1 => "ayer".to_string(),
        d if d < 60 => format!("hace {d} días"),
        d if d < 730 => format!("hace {} meses", d / 30),
        d => format!("hace {} años", d / 365),
    }
}

/// `last_used_days`: Spotlight or iclear's focus record (strong signal).
/// `data_days`: newest change to the app's data in ~/Library (weak signal:
/// background agents also write there). `added_days` resets on every update.
fn verdict(running: bool, last_used_days: Option<u64>, data_days: Option<u64>, added_days: Option<u64>) -> (UsageVerdict, String) {
    if running {
        return (UsageVerdict::Active, "Está abierta ahora".into());
    }
    match (last_used_days, data_days, added_days) {
        (Some(d), _, _) if d <= ACTIVE_DAYS => (UsageVerdict::Active, format!("La usaste {}", ago(d))),
        (Some(d), _, _) if d <= OCCASIONAL_DAYS => (UsageVerdict::Occasional, format!("La usaste {}", ago(d))),
        (Some(d), _, _) => (UsageVerdict::Unused, format!("No la abres {}", ago(d))),
        (None, Some(d), _) if d <= OCCASIONAL_DAYS => (
            UsageVerdict::Occasional,
            format!("No hay registro de que la abrieras, pero sus datos cambiaron {} (puede ser un proceso en segundo plano)", ago(d)),
        ),
        (None, Some(d), _) => (UsageVerdict::Unused, format!("Sin registro de uso y sus datos no cambian {}", ago(d).replacen("hace", "desde hace", 1))),
        (None, None, Some(a)) if a >= ACTIVE_DAYS => (UsageVerdict::Unused, format!("No hay registro de que la hayas abierto desde que llegó, {}", ago(a))),
        (None, None, Some(_)) => (UsageVerdict::Unknown, "Instalada o actualizada hace poco; todavía no hay datos de uso".into()),
        (None, None, None) => (UsageVerdict::Unknown, "Spotlight no tiene datos de uso de esta app".into()),
    }
}

fn running_bundles() -> HashSet<PathBuf> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::Always));
    sys.processes().values().filter_map(|p| p.exe().and_then(outermost_app)).collect()
}

/// Bundle ids of every app Spotlight knows about (anywhere on disk).
fn known_bundle_ids() -> HashSet<String> {
    let out = Command::new("/usr/bin/mdfind")
        .args(["-attr", "kMDItemCFBundleIdentifier", "kMDItemContentType == 'com.apple.application-bundle'"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    out.lines()
        .filter_map(|l| l.rsplit_once("kMDItemCFBundleIdentifier = ").map(|(_, id)| id.trim().to_lowercase()))
        .filter(|id| id != "(null)")
        .collect()
}

fn reverse_dns(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() < 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        return None;
    }
    let tld = parts[0];
    if !(2..=6).contains(&tld.len()) || !tld.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    Some(name.to_lowercase())
}

fn vendor(id: &str) -> String {
    id.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Leftovers in ~/Library named after a bundle id that no installed app owns.
/// Conservative: skips Apple, anything from a vendor that still has an app
/// installed, and anything touched in the last 30 days.
pub fn find_orphans(home: &Path, installed: &HashSet<String>, launch_labels: &HashSet<String>) -> Vec<OrphanGroup> {
    let installed_vendors: HashSet<String> = installed.iter().map(|i| vendor(i)).collect();
    let owned = |id: &str| {
        installed.iter().any(|i| id.starts_with(i.as_str()) || i.starts_with(id))
            || installed_vendors.contains(&vendor(id))
            || launch_labels.iter().any(|l| l.to_lowercase().starts_with(id))
    };
    let mut groups: HashMap<String, Vec<(PathBuf, &'static str)>> = HashMap::new();
    for (dir, label) in APP_DATA_DIRS.iter().filter(|(d, _)| *d != "Library/LaunchAgents") {
        let Ok(rd) = std::fs::read_dir(home.join(dir)) else { continue };
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            let Some(id) = reverse_dns(strip_known_ext(&fname)) else { continue };
            if id.starts_with("com.apple.") || id.starts_with("group.") || id.starts_with("systemgroup.") || id.starts_with("com.iclear") || owned(&id) {
                continue;
            }
            groups.entry(id).or_default().push((e.path(), label));
        }
    }
    let mut out: Vec<OrphanGroup> = groups
        .into_par_iter()
        .filter_map(|(bundle_id, paths)| {
            let modified = paths
                .iter()
                .filter_map(|(p, _)| days_since(std::fs::symlink_metadata(p).and_then(|m| m.modified())))
                .min();
            if modified.map(|d| d < ORPHAN_MIN_AGE_DAYS).unwrap_or(true) {
                return None;
            }
            let leftovers: Vec<AppLeftover> = paths
                .into_iter()
                .map(|(p, label)| AppLeftover { size_bytes: path_size(&p).0, path: p.to_string_lossy().to_string(), kind: label.to_string() })
                .collect();
            let total_bytes = leftovers.iter().map(|l| l.size_bytes).sum();
            Some(OrphanGroup { bundle_id, leftovers, total_bytes, modified_days_ago: modified })
        })
        .collect();
    out.sort_by(|a, b| b.total_bytes.cmp(&a.total_bytes));
    out
}

/// Full scan. `focus` is iclear's own front-app record (bundle id → unix secs).
pub fn scan(home: &Path, focus: &HashMap<String, u64>) -> AppsReport {
    let running = running_bundles();
    let agents = crate::launchd::list();
    let labels: HashSet<String> = agents.iter().map(|a| a.label.clone()).collect();
    let now = now_secs();

    let bundles = installed_bundles(home);
    let all_leftovers = assign_leftovers(&DataIndex::build(home), &bundles);

    let mut apps: Vec<InstalledApp> = bundles
        .par_iter()
        .zip(all_leftovers)
        .map(|((path, meta), leftovers)| {
            let spot = spotlight(path);
            let focus_secs = meta.id.as_ref().and_then(|id| focus.get(id)).copied();
            let last_used = spot.last_used.max(focus_secs);
            let last_used_days = last_used.map(|t| now.saturating_sub(t) / 86400);
            let added_days = spot.added.map(|t| now.saturating_sub(t) / 86400);
            let is_running = running.contains(path);

            let path_str = path.to_string_lossy().to_string();
            let id_lower = meta.id.as_deref().unwrap_or("").to_lowercase();
            let launches_at_login = agents.iter().any(|a| {
                a.program.as_deref().map(|p| p.starts_with(&path_str)).unwrap_or(false)
                    || (!id_lower.is_empty() && a.label.to_lowercase().starts_with(&id_lower))
            });
            let leftovers_bytes = leftovers.iter().map(|l| l.size_bytes).sum();
            let data_days = leftovers
                .iter()
                .filter(|l| l.kind != "Registros")
                .filter_map(|l| days_since(std::fs::symlink_metadata(&l.path).and_then(|m| m.modified())))
                .min();
            let (verdict, reason) = verdict(is_running, last_used_days, data_days, added_days);
            let protected = id_lower == "com.apple.safari" || id_lower.starts_with("com.iclear");
            let removable = !protected
                && !is_running
                && writable(path)
                && path.parent().map(writable).unwrap_or(false);

            InstalledApp {
                path: path_str,
                name: meta.name.clone(),
                bundle_id: meta.id.clone(),
                version: meta.version.clone(),
                size_bytes: path_size(path).0,
                leftovers,
                leftovers_bytes,
                last_used_days_ago: last_used_days,
                use_count: spot.use_count,
                added_days_ago: added_days,
                is_running,
                is_electron: meta.electron,
                from_app_store: path.join("Contents/_MASReceipt").exists(),
                launches_at_login,
                verdict,
                reason,
                removable,
            }
        })
        .collect();
    apps.sort_by(|a, b| (b.size_bytes + b.leftovers_bytes).cmp(&(a.size_bytes + a.leftovers_bytes)));

    let mut installed = known_bundle_ids();
    installed.extend(apps.iter().filter_map(|a| a.bundle_id.as_ref().map(|i| i.to_lowercase())));
    let orphans = find_orphans(home, &installed, &labels);

    AppsReport {
        unused_bytes: apps.iter().filter(|a| a.verdict == UsageVerdict::Unused).map(|a| a.size_bytes + a.leftovers_bytes).sum(),
        orphans_bytes: orphans.iter().map(|o| o.total_bytes).sum(),
        apps,
        orphans,
    }
}

/// Preview items to uninstall one app: the bundle plus its leftovers.
pub fn uninstall_items(home: &Path, app_path: &Path) -> Result<Vec<PreviewItem>, String> {
    let bundles = installed_bundles(home);
    let idx = bundles.iter().position(|(p, _)| p == app_path).ok_or("Solo se desinstalan apps de /Applications o ~/Applications")?;
    let meta = &bundles[idx].1;
    if running_bundles().contains(app_path) {
        return Err(format!("{} está abierta. Ciérrala antes de desinstalarla.", meta.name));
    }
    let mut items = vec![PreviewItem {
        path: app_path.to_string_lossy().to_string(),
        size_bytes: path_size(app_path).0,
        is_dir: true,
        category_id: "app".into(),
        modified_days_ago: None,
    }];
    // Another copy with the same bundle id still uses the same data: remove only the bundle.
    let shared = meta.id.is_some() && bundles.iter().enumerate().any(|(k, (_, m))| k != idx && m.id == meta.id);
    if !shared {
        let leftovers = assign_leftovers(&DataIndex::build(home), &bundles).swap_remove(idx);
        items.extend(leftovers.into_iter().map(|l| PreviewItem {
            is_dir: Path::new(&l.path).is_dir(),
            path: l.path,
            size_bytes: l.size_bytes,
            category_id: "app_leftover".into(),
            modified_days_ago: None,
        }));
    }
    Ok(items)
}

/// Preview items for the orphan groups the user picked (re-scanned, never trusted from the UI).
pub fn orphan_items(home: &Path, bundle_ids: &[String]) -> Vec<PreviewItem> {
    let mut installed = known_bundle_ids();
    installed.extend(installed_bundles(home).into_iter().filter_map(|(_, m)| m.id.map(|i| i.to_lowercase())));
    let labels: HashSet<String> = crate::launchd::list().into_iter().map(|a| a.label).collect();
    let wanted: HashSet<String> = bundle_ids.iter().map(|s| s.to_lowercase()).collect();
    find_orphans(home, &installed, &labels)
        .into_iter()
        .filter(|g| wanted.contains(&g.bundle_id))
        .flat_map(|g| g.leftovers)
        .map(|l| PreviewItem {
            is_dir: Path::new(&l.path).is_dir(),
            path: l.path,
            size_bytes: l.size_bytes,
            category_id: "orphan".into(),
            modified_days_ago: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn md_dates() {
        assert_eq!(parse_md_date("1970-01-01 00:00:00 +0000"), Some(0));
        assert_eq!(parse_md_date("2026-11-15 19:34:01 +0000"), Some(1794771241));
        assert_eq!(parse_md_date("2026-11-15 16:34:01 -0300"), Some(1794771241));
        assert_eq!(parse_md_date("NULL"), None);
    }

    #[test]
    fn verdicts() {
        assert_eq!(verdict(true, Some(400), None, None).0, UsageVerdict::Active);
        assert_eq!(verdict(false, Some(3), Some(900), None).0, UsageVerdict::Active);
        assert_eq!(verdict(false, Some(60), None, None).0, UsageVerdict::Occasional);
        assert_eq!(verdict(false, Some(200), Some(1), None).0, UsageVerdict::Unused, "real use beats agent writes");
        assert_eq!(verdict(false, None, Some(10), Some(2)).0, UsageVerdict::Occasional);
        assert_eq!(verdict(false, None, Some(300), Some(2)).0, UsageVerdict::Unused);
        assert_eq!(verdict(false, None, None, Some(100)).0, UsageVerdict::Unused);
        assert_eq!(verdict(false, None, None, Some(5)).0, UsageVerdict::Unknown);
    }

    fn meta(id: &str, name: &str) -> BundleMeta {
        BundleMeta { id: Some(id.into()), name: name.into(), version: None, executable: None, background: false, electron: false, system: false }
    }

    #[test]
    fn ambiguous_leftovers_are_not_assigned() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let support = home.join("Library/Application Support");
        for d in ["Claude", "com.anthropic.claudefordesktop", "Shared"] {
            fs::create_dir_all(support.join(d)).unwrap();
        }
        let apps = vec![
            (PathBuf::from("/Applications/Claude.app"), meta("com.anthropic.claudefordesktop", "Claude")),
            (PathBuf::from("/Applications/Claude URL Handler.app"), meta("com.anthropic.url", "Claude URL Handler")),
            (PathBuf::from("/Applications/Shared.app"), meta("com.a.shared", "Shared")),
            (PathBuf::from("/Applications/Shared2.app"), meta("com.b.shared", "Shared")),
        ];
        let out = assign_leftovers(&DataIndex::build(home), &apps);
        let names = |k: usize| out[k].iter().map(|l| Path::new(&l.path).file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>();
        let mut claude = names(0);
        claude.sort();
        assert_eq!(claude, vec!["Claude", "com.anthropic.claudefordesktop"]);
        assert!(names(1).is_empty(), "the URL handler must not get Claude's data");
        assert!(names(2).is_empty() && names(3).is_empty(), "same name in two apps: nobody gets it");
    }

    #[test]
    fn orphans_are_conservative() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let support = home.join("Library/Application Support");
        let prefs = home.join("Library/Preferences");
        fs::create_dir_all(&support).unwrap();
        fs::create_dir_all(&prefs).unwrap();
        for d in ["com.gone.app", "com.google.Keystone", "com.apple.Safari", "Slack", "com.live.app"] {
            fs::create_dir_all(support.join(d)).unwrap();
        }
        fs::write(prefs.join("com.gone.app.plist"), b"x").unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(90 * 86400);
        for p in [support.join("com.gone.app"), prefs.join("com.gone.app.plist"), support.join("com.google.Keystone"), support.join("com.live.app")] {
            fs::File::options().write(true).open(&p).or_else(|_| fs::File::open(&p)).unwrap().set_modified(old).ok();
        }
        // set_modified needs a writable handle for files; for dirs use utimes via filetime-less trick.
        for p in [support.join("com.gone.app"), support.join("com.google.Keystone"), support.join("com.live.app")] {
            let c = std::ffi::CString::new(p.to_string_lossy().as_bytes()).unwrap();
            let t = libc::timeval { tv_sec: (now_secs() - 90 * 86400) as i64, tv_usec: 0 };
            unsafe { libc::utimes(c.as_ptr(), [t, t].as_ptr()) };
        }
        let installed: HashSet<String> = ["com.google.chrome".to_string(), "com.live.app".to_string()].into();
        let orphans = find_orphans(home, &installed, &HashSet::new());
        let ids: Vec<&str> = orphans.iter().map(|o| o.bundle_id.as_str()).collect();
        assert_eq!(ids, vec!["com.gone.app"], "only the truly orphaned id");
        assert_eq!(orphans[0].leftovers.len(), 2);
    }
}
