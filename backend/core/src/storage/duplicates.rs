//! Duplicate file finder: group by size → hash of the first 64 KB → full blake3.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use iclear_protocol::DuplicateGroup;
use jwalk::WalkDir;
use rayon::prelude::*;

fn hash_prefix(path: &Path) -> Option<blake3::Hash> {
    let mut f = File::open(path).ok()?;
    let mut buf = vec![0u8; 64 * 1024];
    let n = f.read(&mut buf).ok()?;
    Some(blake3::hash(&buf[..n]))
}

fn hash_full(path: &Path) -> Option<blake3::Hash> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(path).ok()?).ok()?;
    Some(hasher.finalize())
}

fn regroup<K, F>(groups: Vec<Vec<PathBuf>>, key: F) -> Vec<Vec<PathBuf>>
where
    K: std::hash::Hash + Eq + Send,
    F: Fn(&Path) -> Option<K> + Sync,
{
    groups
        .into_par_iter()
        .flat_map(|group| {
            let mut by: HashMap<K, Vec<PathBuf>> = HashMap::new();
            for p in group {
                if let Some(k) = key(&p) {
                    by.entry(k).or_default().push(p);
                }
            }
            by.into_values().filter(|g| g.len() > 1).collect::<Vec<_>>()
        })
        .collect()
}

pub fn find(root: &Path, min_bytes: u64, skip: &[PathBuf]) -> Vec<DuplicateGroup> {
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    // Hard links share an inode: they are not real duplicates.
    let mut seen_inodes = std::collections::HashSet::new();
    let skip = skip.to_vec();
    let walker = WalkDir::new(root).skip_hidden(true).follow_links(false).process_read_dir(move |_, _, _, children| {
        children.retain(|c| c.as_ref().map(|e| !skip.iter().any(|s| e.path().starts_with(s))).unwrap_or(false));
    });
    for entry in walker.into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() < min_bytes || !seen_inodes.insert((meta.dev(), meta.ino())) {
            continue;
        }
        by_size.entry(meta.len()).or_default().push(entry.path());
    }
    let candidates: Vec<Vec<PathBuf>> = by_size.into_values().filter(|g| g.len() > 1).collect();
    let candidates = regroup(candidates, hash_prefix);
    let groups = regroup(candidates, hash_full);

    let mut out: Vec<DuplicateGroup> = groups
        .into_iter()
        .filter_map(|mut paths| {
            paths.sort();
            let size = std::fs::metadata(&paths[0]).ok()?.len();
            let hash = hash_full(&paths[0])?.to_hex().to_string();
            Some(DuplicateGroup { hash, size_bytes: size, paths: paths.iter().map(|p| p.to_string_lossy().to_string()).collect() })
        })
        .collect();
    // Most wasted space first.
    out.sort_by_key(|g| std::cmp::Reverse(g.size_bytes * (g.paths.len() as u64 - 1)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_real_duplicates_only() {
        let dir = tempfile::tempdir().unwrap();
        let a = vec![7u8; 200_000];
        let mut b = a.clone();
        *b.last_mut().unwrap() = 8; // same size and prefix, different content
        fs::write(dir.path().join("one.bin"), &a).unwrap();
        fs::write(dir.path().join("two.bin"), &a).unwrap();
        fs::write(dir.path().join("almost.bin"), &b).unwrap();
        fs::hard_link(dir.path().join("one.bin"), dir.path().join("link.bin")).unwrap();

        let groups = find(dir.path(), 1000, &[]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), 2);
    }
}
