//! Permanent deletion with safety guards.
//!
//! Flow: `PreviewStore::create` registers exactly what was shown to the user
//! and returns a one-time token → `PreviewStore::execute` deletes only paths
//! that belong to that preview and pass `Guard::check`. Every deletion is
//! appended to ~/Library/Logs/iclear/deletions.log.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iclear_protocol::{CleanupPreview, DeleteReport, PreviewItem};

use super::categories;
use super::scan::path_size;

const TOKEN_TTL: Duration = Duration::from_secs(15 * 60);

/// Relative to $HOME: never deleted, nor anything inside them.
const HOME_DENY: &[&str] = &[
    "Library/Keychains",
    "Library/Mobile Documents",
    "Library/Mail",
    "Library/Messages",
    "Library/Photos",
    "Library/Application Support/MobileSync",
    "Library/Application Support/AddressBook",
    "Library/Calendars",
    "Library/Logs/iclear",
    "Library/CloudStorage",
    "Library/Group Containers",
    "Pictures/Photos Library.photoslibrary",
    ".ssh",
    ".gnupg",
];

pub struct Guard {
    home: PathBuf,
    category_roots: Vec<PathBuf>,
}

impl Guard {
    pub fn new(home: PathBuf) -> Self {
        let home = home.canonicalize().unwrap_or(home);
        let category_roots = categories::all_roots(&home);
        Self { home, category_roots }
    }

    /// Ok if `path` may be permanently deleted.
    /// - must be absolute, without `..`, strictly inside $HOME and outside the deny list;
    /// - its parent must not be reached through a symlink;
    /// - directories only when strictly inside a cleanup category root;
    /// - plain files anywhere else in $HOME (large files, duplicates).
    pub fn check(&self, path: &Path) -> Result<(), String> {
        if self.home.components().count() < 3 {
            return Err("HOME no es válido; no se borra nada".into());
        }
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(format!("ruta no válida: {}", path.display()));
        }
        if !path.starts_with(&self.home) || path == self.home {
            return Err(format!("fuera de tu carpeta de usuario: {}", path.display()));
        }
        for deny in HOME_DENY {
            if path.starts_with(self.home.join(deny)) {
                return Err(format!("ruta protegida: {}", path.display()));
            }
        }
        if self.category_roots.iter().any(|r| r == path) {
            return Err(format!("no se borra la carpeta raíz, solo su contenido: {}", path.display()));
        }
        let parent = path.parent().ok_or("sin carpeta padre")?;
        let real_parent = parent.canonicalize().map_err(|e| format!("{}: {e}", parent.display()))?;
        if real_parent != parent {
            return Err(format!("la ruta pasa por un symlink: {}", path.display()));
        }
        let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.is_dir() {
            let inside_root = self.category_roots.iter().any(|r| path.starts_with(r) && path != r);
            if !inside_root {
                return Err(format!("solo se borran carpetas dentro de categorías de limpieza: {}", path.display()));
            }
        } else {
            // Top-level home dirs are dirs, so they're already covered; protect dotfiles in $HOME root.
            if path.parent() == Some(self.home.as_path())
                && path.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(false)
            {
                return Err(format!("archivo de configuración protegido: {}", path.display()));
            }
        }
        Ok(())
    }
}

impl Guard {
    pub fn check_for(&self, path: &Path, app_removal: bool) -> Result<(), String> {
        if app_removal {
            self.check_app_removal(path)
        } else {
            self.check(path)
        }
    }

    /// Stricter rule set for uninstalling apps. Only two shapes are accepted:
    /// - an app bundle `X.app` directly in /Applications, a vendor folder
    ///   inside it, or ~/Applications (never Safari, never a symlink);
    /// - a direct child of one of the per-app folders in ~/Library
    ///   (`apps::APP_DATA_DIRS`), outside the deny list.
    pub fn check_app_removal(&self, path: &Path) -> Result<(), String> {
        if self.home.components().count() < 3 {
            return Err("HOME no es válido; no se borra nada".into());
        }
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(format!("ruta no válida: {}", path.display()));
        }
        let parent = path.parent().ok_or("sin carpeta padre")?;
        let real_parent = parent.canonicalize().map_err(|e| format!("{}: {e}", parent.display()))?;
        if real_parent != parent {
            return Err(format!("la ruta pasa por un symlink: {}", path.display()));
        }
        let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;

        // Decided by location, not extension: "com.foo.app" is a common data folder name.
        let in_app_data = crate::apps::APP_DATA_DIRS.iter().any(|(d, _)| parent == self.home.join(d));
        if !in_app_data && path.extension().map(|e| e == "app").unwrap_or(false) {
            let apps = Path::new("/Applications");
            let in_apps = parent == apps
                || parent == self.home.join("Applications")
                || (parent.parent() == Some(apps) && parent.extension().map(|e| e != "app").unwrap_or(true));
            if !in_apps {
                return Err(format!("solo se desinstalan apps de /Applications o ~/Applications: {}", path.display()));
            }
            if !meta.is_dir() {
                return Err(format!("no es un paquete de app: {}", path.display()));
            }
            let id = crate::memory::read_bundle(path).and_then(|m| m.id).ok_or_else(|| format!("no es una app válida: {}", path.display()))?;
            if id.eq_ignore_ascii_case("com.apple.Safari") || id.to_lowercase().starts_with("com.iclear") {
                return Err(format!("app protegida: {}", path.display()));
            }
            return Ok(());
        }

        if !path.starts_with(&self.home) {
            return Err(format!("fuera de tu carpeta de usuario: {}", path.display()));
        }
        for deny in HOME_DENY {
            if path.starts_with(self.home.join(deny)) {
                return Err(format!("ruta protegida: {}", path.display()));
            }
        }
        if !in_app_data {
            return Err(format!("no es una carpeta de datos de app: {}", path.display()));
        }
        Ok(())
    }
}

/// Removes a path permanently. Symlinks are removed, never followed
/// (`remove_dir_all` does not follow links either).
fn remove(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

fn random_token() -> String {
    let mut buf = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn log_line(home: &Path, line: &str) {
    let dir = home.join("Library/Logs/iclear");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("deletions.log")) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{ts}\t{line}");
    }
}

struct Pending {
    created: Instant,
    /// path → (size, is_app_removal)
    items: HashMap<PathBuf, (u64, bool)>,
}

pub struct PreviewStore {
    home: PathBuf,
    pending: Mutex<HashMap<String, Pending>>,
}

impl PreviewStore {
    pub fn new(home: PathBuf) -> Self {
        let home = home.canonicalize().unwrap_or(home);
        Self { home, pending: Mutex::new(HashMap::new()) }
    }

    /// Registers the given items (already sized) and returns the preview with its token.
    /// Items that fail the guard are dropped here so the UI never offers them.
    pub fn create(&self, items: Vec<PreviewItem>) -> CleanupPreview {
        self.register(items, false)
    }

    /// Preview for uninstalling apps / removing app leftovers. These paths are
    /// computed by the backend and checked with the stricter `check_app_removal`.
    pub fn create_app_removal(&self, items: Vec<PreviewItem>) -> CleanupPreview {
        self.register(items, true)
    }

    fn register(&self, items: Vec<PreviewItem>, app_removal: bool) -> CleanupPreview {
        let guard = Guard::new(self.home.clone());
        let items: Vec<PreviewItem> = items.into_iter().filter(|i| guard.check_for(Path::new(&i.path), app_removal).is_ok()).collect();
        let token = random_token();
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|_, p| p.created.elapsed() < TOKEN_TTL);
        pending.insert(
            token.clone(),
            Pending {
                created: Instant::now(),
                items: items.iter().map(|i| (PathBuf::from(&i.path), (i.size_bytes, app_removal))).collect(),
            },
        );
        CleanupPreview { token, total_bytes: items.iter().map(|i| i.size_bytes).sum(), items }
    }

    /// Builds a preview for arbitrary user-picked paths (large files, duplicates).
    pub fn create_for_paths(&self, paths: &[String]) -> CleanupPreview {
        let items = paths
            .iter()
            .filter_map(|p| {
                let meta = std::fs::symlink_metadata(p).ok()?;
                Some(PreviewItem {
                    path: p.clone(),
                    size_bytes: path_size(Path::new(p)).0,
                    is_dir: meta.is_dir(),
                    category_id: "manual".into(),
                    modified_days_ago: super::scan::days_since(meta.modified()),
                })
            })
            .collect();
        self.create(items)
    }

    /// Deletes `selected` (must be a subset of the preview). Consumes the token.
    pub fn execute(&self, token: &str, selected: &[String]) -> Result<DeleteReport, String> {
        let pending = self.pending.lock().unwrap().remove(token).ok_or("La vista previa expiró. Vuelve a escanear.")?;
        if pending.created.elapsed() > TOKEN_TTL {
            return Err("La vista previa expiró. Vuelve a escanear.".into());
        }
        let guard = Guard::new(self.home.clone());
        let mut report = DeleteReport::default();
        let mut seen = HashSet::new();
        for s in selected {
            let path = PathBuf::from(s);
            if !seen.insert(path.clone()) {
                continue;
            }
            let Some(&(size, app_removal)) = pending.items.get(&path) else {
                report.errors.push(format!("no estaba en la vista previa: {s}"));
                continue;
            };
            if let Err(e) = guard.check_for(&path, app_removal) {
                report.errors.push(e);
                continue;
            }
            match remove(&path) {
                Ok(()) => {
                    report.deleted += 1;
                    report.freed_bytes += size;
                    log_line(&self.home, &format!("deleted\t{size}\t{s}"));
                }
                Err(e) => {
                    log_line(&self.home, &format!("error\t{s}\t{e}"));
                    report.errors.push(format!("{s}: {e}"));
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fake_home() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().canonicalize().unwrap();
        fs::create_dir_all(home.join("Library/Caches/com.example.app")).unwrap();
        fs::write(home.join("Library/Caches/com.example.app/blob"), vec![0u8; 10_000]).unwrap();
        fs::create_dir_all(home.join("Library/Keychains")).unwrap();
        fs::write(home.join("Library/Keychains/login.keychain-db"), b"secret").unwrap();
        fs::create_dir_all(home.join("Documents/project")).unwrap();
        fs::write(home.join("Documents/movie.mov"), vec![0u8; 10_000]).unwrap();
        fs::write(home.join(".zshrc"), b"x").unwrap();
        (dir, home)
    }

    #[test]
    fn guard_rules() {
        let (_d, home) = fake_home();
        let g = Guard::new(home.clone());
        assert!(g.check(&home.join("Library/Caches/com.example.app")).is_ok());
        assert!(g.check(&home.join("Documents/movie.mov")).is_ok(), "plain files are allowed");
        assert!(g.check(&home.join("Library/Caches")).is_err(), "category root itself");
        assert!(g.check(&home.join("Library/Keychains/login.keychain-db")).is_err());
        assert!(g.check(&home.join("Documents/project")).is_err(), "dirs outside categories");
        assert!(g.check(&home.join(".zshrc")).is_err());
        assert!(g.check(&home).is_err());
        assert!(g.check(Path::new("/System/Library")).is_err());
        assert!(g.check(&home.join("Library/Caches/../Keychains")).is_err());
    }

    #[test]
    fn symlinked_parent_is_rejected() {
        let (_d, home) = fake_home();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("victim"), b"x").unwrap();
        std::os::unix::fs::symlink(outside.path(), home.join("Library/Caches/escape")).unwrap();
        let g = Guard::new(home.clone());
        assert!(g.check(&home.join("Library/Caches/escape/victim")).is_err());
        // The link itself may be removed (it is not followed).
        assert!(g.check(&home.join("Library/Caches/escape")).is_ok());
        remove(&home.join("Library/Caches/escape")).unwrap();
        assert!(outside.path().join("victim").exists());
    }

    #[test]
    fn app_removal_rules() {
        let (_d, home) = fake_home();
        fs::create_dir_all(home.join("Library/Application Support/com.gone.app")).unwrap();
        fs::create_dir_all(home.join("Applications/Foo.app/Contents")).unwrap();
        fs::write(
            home.join("Applications/Foo.app/Contents/Info.plist"),
            r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.foo</string></dict></plist>"#,
        )
        .unwrap();
        let g = Guard::new(home.clone());
        assert_eq!(g.check_app_removal(&home.join("Library/Application Support/com.gone.app")), Ok(()));
        assert!(g.check_app_removal(&home.join("Applications/Foo.app")).is_ok());
        assert!(g.check_app_removal(&home.join("Library/Application Support")).is_err(), "the folder itself");
        assert!(g.check_app_removal(&home.join("Library/Keychains/login.keychain-db")).is_err());
        assert!(g.check_app_removal(&home.join("Documents/movie.mov")).is_err(), "not app data");
        assert!(g.check_app_removal(&home.join("Library/Group Containers")).is_err());
        assert!(g.check_app_removal(Path::new("/System/Applications/Notes.app")).is_err());
        // App-removal previews can't be executed with the normal guard and vice versa.
        assert!(g.check(&home.join("Library/Application Support/com.gone.app")).is_err());
    }

    #[test]
    fn token_flow() {
        let (_d, home) = fake_home();
        let store = PreviewStore::new(home.clone());
        let items = categories::find("user_caches").unwrap().items(&home);
        assert_eq!(items.len(), 1);
        let preview = store.create(items);
        let target = home.join("Library/Caches/com.example.app").to_string_lossy().to_string();
        let not_previewed = home.join("Documents/movie.mov").to_string_lossy().to_string();

        let report = store.execute(&preview.token, &[target.clone(), not_previewed.clone()]).unwrap();
        assert_eq!(report.deleted, 1);
        assert_eq!(report.errors.len(), 1);
        assert!(!Path::new(&target).exists());
        assert!(Path::new(&not_previewed).exists());
        assert!(store.execute(&preview.token, &[target]).is_err(), "token is single use");
        assert!(home.join("Library/Logs/iclear/deletions.log").exists());
    }
}
