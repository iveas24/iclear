//! Cleanup categories: well-known folders whose contents can be removed.
//! See docs/CLEANUP_RULES.md for the reasoning behind each entry.

use std::path::{Path, PathBuf};

use iclear_protocol::{CleanupCategory, PreviewItem, Risk};
use rayon::prelude::*;

use super::scan::{days_since, path_size};

pub struct CategoryDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Relative to $HOME. The roots themselves are never deleted, only their children.
    pub roots: &'static [&'static str],
    pub risk: Risk,
    pub selected_by_default: bool,
    /// Only children older than this many days are listed.
    pub min_age_days: Option<u64>,
}

pub const CATEGORIES: &[CategoryDef] = &[
    CategoryDef {
        id: "user_caches",
        name: "Cachés de apps",
        description: "Archivos temporales que las apps regeneran solas (~/Library/Caches).",
        roots: &["Library/Caches"],
        risk: Risk::Safe,
        selected_by_default: true,
        min_age_days: None,
    },
    CategoryDef {
        id: "user_logs",
        name: "Logs",
        description: "Registros de apps y reportes de fallos antiguos.",
        roots: &["Library/Logs", "Library/Logs/DiagnosticReports"],
        risk: Risk::Safe,
        selected_by_default: true,
        min_age_days: None,
    },
    CategoryDef {
        id: "xcode_derived",
        name: "Xcode DerivedData",
        description: "Builds intermedios de Xcode. Se reconstruyen al compilar.",
        roots: &["Library/Developer/Xcode/DerivedData"],
        risk: Risk::Safe,
        selected_by_default: true,
        min_age_days: None,
    },
    CategoryDef {
        id: "xcode_device_support",
        name: "Soporte de dispositivos Xcode",
        description: "Símbolos de versiones de iOS/watchOS. Se vuelven a descargar al conectar un dispositivo.",
        roots: &[
            "Library/Developer/Xcode/iOS DeviceSupport",
            "Library/Developer/Xcode/watchOS DeviceSupport",
            "Library/Developer/Xcode/tvOS DeviceSupport",
        ],
        risk: Risk::Moderate,
        selected_by_default: false,
        min_age_days: None,
    },
    CategoryDef {
        id: "xcode_archives",
        name: "Archivos de Xcode",
        description: "Archives de apps ya publicadas (.xcarchive). Revisa antes de borrar.",
        roots: &["Library/Developer/Xcode/Archives"],
        risk: Risk::Review,
        selected_by_default: false,
        min_age_days: None,
    },
    CategoryDef {
        id: "simulator_caches",
        name: "Cachés de simuladores",
        description: "Cachés de CoreSimulator.",
        roots: &["Library/Developer/CoreSimulator/Caches"],
        risk: Risk::Safe,
        selected_by_default: true,
        min_age_days: None,
    },
    CategoryDef {
        id: "dev_caches",
        name: "Cachés de desarrollo",
        description: "npm, pnpm, bun, pip, cargo, gradle… Se descargan de nuevo cuando hagan falta.",
        roots: &[
            ".npm/_cacache",
            "Library/pnpm/store",
            ".bun/install/cache",
            ".cache",
            ".cargo/registry/cache",
            ".gradle/caches",
        ],
        risk: Risk::Moderate,
        selected_by_default: false,
        min_age_days: None,
    },
    CategoryDef {
        id: "mail_downloads",
        name: "Adjuntos de Mail",
        description: "Copias de adjuntos que Mail guardó al abrirlos.",
        roots: &["Library/Containers/com.apple.mail/Data/Library/Mail Downloads"],
        risk: Risk::Safe,
        selected_by_default: true,
        min_age_days: None,
    },
    CategoryDef {
        id: "trash",
        name: "Papelera",
        description: "Lo que ya está en la Papelera.",
        roots: &[".Trash"],
        risk: Risk::Safe,
        selected_by_default: true,
        min_age_days: None,
    },
    CategoryDef {
        id: "old_downloads",
        name: "Descargas antiguas",
        description: "Elementos en Descargas sin modificar hace más de 90 días.",
        roots: &["Downloads"],
        risk: Risk::Review,
        selected_by_default: false,
        min_age_days: Some(90),
    },
];

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn find(id: &str) -> Option<&'static CategoryDef> {
    CATEGORIES.iter().find(|c| c.id == id)
}

impl CategoryDef {
    pub fn root_paths(&self, home: &Path) -> Vec<PathBuf> {
        self.roots.iter().map(|r| home.join(r)).collect()
    }

    /// Lists the deletable children of every root, with sizes.
    pub fn items(&self, home: &Path) -> Vec<PreviewItem> {
        let roots = self.root_paths(home);
        let mut paths: Vec<PathBuf> = Vec::new();
        for root in &roots {
            let Ok(rd) = std::fs::read_dir(root) else { continue };
            for entry in rd.flatten() {
                let p = entry.path();
                // A nested root (e.g. Logs/DiagnosticReports) is handled on its own.
                if roots.iter().any(|r| r == &p) {
                    continue;
                }
                // Never touch our own deletion log.
                if p.ends_with("Library/Logs/iclear") {
                    continue;
                }
                if p.file_name().map(|n| n == ".DS_Store" || n == ".localized").unwrap_or(false) {
                    continue;
                }
                paths.push(p);
            }
        }
        let mut items: Vec<PreviewItem> = paths
            .par_iter()
            .filter_map(|p| {
                let meta = std::fs::symlink_metadata(p).ok()?;
                let age = days_since(meta.modified());
                if let Some(min) = self.min_age_days {
                    if age.unwrap_or(0) < min {
                        return None;
                    }
                }
                let (size, _) = path_size(p);
                Some(PreviewItem {
                    path: p.to_string_lossy().to_string(),
                    size_bytes: size,
                    is_dir: meta.is_dir(),
                    category_id: self.id.to_string(),
                    modified_days_ago: age,
                })
            })
            .collect();
        items.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
        items
    }

    pub fn summary(&self, home: &Path) -> CleanupCategory {
        let items = self.items(home);
        CleanupCategory {
            id: self.id.to_string(),
            name: self.name.to_string(),
            description: self.description.to_string(),
            roots: self.root_paths(home).iter().map(|p| p.to_string_lossy().to_string()).collect(),
            size_bytes: items.iter().map(|i| i.size_bytes).sum(),
            item_count: items.len(),
            risk: self.risk,
            requires_helper: false,
            selected_by_default: self.selected_by_default,
        }
    }
}

/// Summaries of every category, computed in parallel.
pub fn summaries(home: &Path) -> Vec<CleanupCategory> {
    CATEGORIES.par_iter().map(|c| c.summary(home)).collect()
}

/// All category roots (absolute). Directories may only be deleted when they
/// are strictly inside one of these.
pub fn all_roots(home: &Path) -> Vec<PathBuf> {
    CATEGORIES.iter().flat_map(|c| c.root_paths(home)).collect()
}
