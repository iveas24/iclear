//! Sync status of files in cloud drives, read through Foundation
//! (`NSURL` ubiquitous resource keys) and the `SF_DATALESS` stat flag.
//!
//! Every File Provider domain (Google Drive, OneDrive, Dropbox, Proton…) and
//! iCloud Drive answer these keys, so one code path covers all providers.

use std::os::macos::fs::MetadataExt as _;
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSFileManager, NSNumber, NSString, NSURLResourceKey, NSURL};

/// `SF_DATALESS` from <sys/stat.h>: the file (or folder) is a placeholder
/// whose content lives only in the cloud.
pub const SF_DATALESS: u32 = 0x4000_0000;

pub fn is_dataless(meta: &std::fs::Metadata) -> bool {
    meta.st_flags() & SF_DATALESS != 0
}

/// Bytes actually allocated on disk.
pub fn on_disk(meta: &std::fs::Metadata) -> u64 {
    meta.st_blocks() * 512
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncFlags {
    /// The provider manages this item (false for plain local folders).
    pub managed: bool,
    pub uploaded: bool,
    pub uploading: bool,
    pub downloading: bool,
    pub conflict: bool,
}

fn url(path: &Path) -> Option<Retained<NSURL>> {
    let s = path.to_str()?;
    Some(NSURL::fileURLWithPath(&NSString::from_str(s)))
}

fn bool_key(url: &NSURL, key: &NSURLResourceKey) -> Option<bool> {
    let mut value: Option<Retained<AnyObject>> = None;
    // SAFETY: `value` receives an autoreleased object of the key's documented
    // type (NSNumber for every key used here).
    unsafe { url.getResourceValue_forKey_error(&mut value, key) }.ok()?;
    let n = value?.downcast::<NSNumber>().ok()?;
    Some(n.boolValue())
}

/// Upload/download flags for one file. Cheap for non-managed paths.
pub fn flags(path: &Path) -> SyncFlags {
    objc2::rc::autoreleasepool(|_| {
        let Some(u) = url(path) else { return SyncFlags::default() };
        // SAFETY: reading Foundation's immutable key constants.
        unsafe {
            use objc2_foundation::*;
            if !bool_key(&u, NSURLIsUbiquitousItemKey).unwrap_or(false) {
                return SyncFlags::default();
            }
            SyncFlags {
                managed: true,
                uploaded: bool_key(&u, NSURLUbiquitousItemIsUploadedKey).unwrap_or(true),
                uploading: bool_key(&u, NSURLUbiquitousItemIsUploadingKey).unwrap_or(false),
                downloading: bool_key(&u, NSURLUbiquitousItemIsDownloadingKey).unwrap_or(false),
                conflict: bool_key(&u, NSURLUbiquitousItemHasUnresolvedConflictsKey).unwrap_or(false),
            }
        }
    })
}

/// Is this file confirmed as uploaded? `None` when the provider does not
/// report it (not a managed item).
pub fn uploaded(path: &Path) -> Option<bool> {
    objc2::rc::autoreleasepool(|_| {
        let u = url(path)?;
        // SAFETY: reading Foundation's immutable key constants.
        unsafe {
            use objc2_foundation::*;
            if !bool_key(&u, NSURLIsUbiquitousItemKey).unwrap_or(false) {
                return None;
            }
            bool_key(&u, NSURLUbiquitousItemIsUploadedKey)
        }
    })
}

fn ns_err(e: Retained<objc2_foundation::NSError>) -> String {
    e.localizedDescription().to_string()
}

/// Asks the provider to download a cloud-only file (asynchronous).
pub fn start_download(path: &Path) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| {
        let u = url(path).ok_or("Ruta no válida")?;
        NSFileManager::defaultManager().startDownloadingUbiquitousItemAtURL_error(&u).map_err(ns_err)
    })
}

/// Removes the local copy of a file that is already in the cloud. The
/// provider refuses files with pending changes.
pub fn evict(path: &Path) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| {
        let u = url(path).ok_or("Ruta no válida")?;
        NSFileManager::defaultManager().evictUbiquitousItemAtURL_error(&u).map_err(ns_err)
    })
}
