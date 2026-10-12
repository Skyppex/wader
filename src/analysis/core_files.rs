//! The core library as files an editor can open. Core modules are built
//! into the compiler, not on disk, so to show one (go to definition), wader
//! writes it once to a cache folder, read-only, named after the exact
//! sources so a newer compiler never shows an older copy.

use std::path::{Path, PathBuf};

use rill::lang::core;

/// The folder the core library is written to.
pub fn dir() -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    // FNV-1a over every module, so each set of sources has its own folder.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, src) in core::FILES {
        for b in name.bytes().chain(src.bytes()) {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    cache.join("rill").join("core").join(format!("{hash:016x}"))
}

/// The file showing core module `name` (as in `"gen/osc"`), written if it
/// is not there yet.
pub fn path(name: &str) -> Option<PathBuf> {
    let src = core::module(name)?;
    let path = dir().join(format!("{name}.rill"));
    if !path.exists() {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, src).ok()?;
        // A copy: edits to it would change nothing the compiler uses.
        let mut perms = std::fs::metadata(&path).ok()?.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(&path, perms);
    }
    Some(path)
}

/// Whether `path` is one of these copies.
pub fn is_copy(path: &Path) -> bool {
    path.starts_with(dir().parent().unwrap_or(Path::new("")))
}
