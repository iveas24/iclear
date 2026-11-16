//! Disk usage scanning: folder sizes and large files.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use iclear_protocol::{DirUsage, LargeFile, UsageReport};
use jwalk::WalkDir;
use rayon::prelude::*;

/// Bytes actually allocated on disk (APFS clones/sparse files report less than `len`).
pub fn allocated(meta: &std::fs::Metadata) -> u64 {
    meta.blocks() * 512
}

pub fn days_since(time: std::io::Result<SystemTime>) -> Option<u64> {
    let t = time.ok()?;
    SystemTime::now().duration_since(t).ok().map(|d| d.as_secs() / 86_400)
}

/// Recursive size of a file or directory. Never follows symlinks.
/// Returns (bytes, number of entries that could not be read).
///
/// Walks serially on purpose: callers already parallelize across items with
/// rayon, and a jwalk walk nested in a busy rayon pool gives up after its
/// busy timeout and silently returns nothing.
pub fn path_size(path: &Path) -> (u64, usize) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return (0, 1),
    };
    if !meta.is_dir() {
        return (allocated(&meta), 0);
    }
    let errors = AtomicUsize::new(0);
    let total = WalkDir::new(path)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(jwalk::Parallelism::Serial)
        .into_iter()
        .filter_map(|e| match e {
            Ok(e) => Some(e),
            Err(_) => {
                errors.fetch_add(1, Ordering::Relaxed);
                None
            }
        })
        .filter(|e| !e.file_type().is_dir())
        .filter_map(|e| e.metadata().ok())
        .map(|m| allocated(&m))
        .sum();
    (total, errors.load(Ordering::Relaxed))
}

/// Size of every direct child of `root`, largest first.
pub fn usage(root: &Path) -> std::io::Result<UsageReport> {
    let entries: Vec<PathBuf> = std::fs::read_dir(root)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    let skipped = AtomicUsize::new(0);
    let mut children: Vec<DirUsage> = entries
        .par_iter()
        .map(|p| {
            let (size, errs) = path_size(p);
            skipped.fetch_add(errs, Ordering::Relaxed);
            DirUsage {
                path: p.to_string_lossy().to_string(),
                name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                size_bytes: size,
                is_dir: std::fs::symlink_metadata(p).map(|m| m.is_dir()).unwrap_or(false),
            }
        })
        .collect();
    children.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    Ok(UsageReport {
        root: root.to_string_lossy().to_string(),
        total_bytes: children.iter().map(|c| c.size_bytes).sum(),
        children,
        skipped_errors: skipped.into_inner(),
    })
}

/// The `limit` biggest files under `root` that are at least `min_bytes`.
pub fn large_files(root: &Path, min_bytes: u64, limit: usize, skip: &[PathBuf]) -> Vec<LargeFile> {
    let mut heap: BinaryHeap<Reverse<(u64, PathBuf)>> = BinaryHeap::new();
    let walker = WalkDir::new(root).skip_hidden(false).follow_links(false).process_read_dir({
        let skip = skip.to_vec();
        move |_, _, _, children| {
            children.retain(|c| c.as_ref().map(|e| !skip.iter().any(|s| e.path().starts_with(s))).unwrap_or(false));
        }
    });
    for entry in walker.into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let size = allocated(&meta);
        if size < min_bytes {
            continue;
        }
        heap.push(Reverse((size, entry.path())));
        if heap.len() > limit {
            heap.pop();
        }
    }
    let mut out: Vec<LargeFile> = heap
        .into_iter()
        .map(|Reverse((size, path))| LargeFile {
            modified_days_ago: std::fs::symlink_metadata(&path).ok().and_then(|m| days_since(m.modified())),
            path: path.to_string_lossy().to_string(),
            size_bytes: size,
        })
        .collect();
    out.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sizes_and_large_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("a")).unwrap();
        fs::write(dir.path().join("a/big.bin"), vec![1u8; 2_000_000]).unwrap();
        fs::write(dir.path().join("small.txt"), b"hola").unwrap();

        let report = usage(dir.path()).unwrap();
        assert_eq!(report.children[0].name, "a");
        assert!(report.children[0].size_bytes >= 2_000_000);

        let big = large_files(dir.path(), 1_000_000, 10, &[]);
        assert_eq!(big.len(), 1);
        assert!(big[0].path.ends_with("big.bin"));

        let none = large_files(dir.path(), 1_000_000, 10, &[dir.path().join("a")]);
        assert!(none.is_empty());
    }
}
