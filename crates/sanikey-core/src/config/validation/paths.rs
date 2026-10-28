//! Home and configuration-relative path handling.

use std::path::{Path, PathBuf};

pub(in crate::config) fn expand_user_path(value: &str) -> PathBuf {
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        if let Some(home) = home {
            if value == "~" {
                return home;
            }
            return home.join(&value[2..]);
        }
    }
    PathBuf::from(value)
}

pub(in crate::config) fn resolve_path(value: &str, base_dir: &Path) -> PathBuf {
    let path = expand_user_path(value);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}
