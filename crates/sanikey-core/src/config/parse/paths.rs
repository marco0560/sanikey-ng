//! Filesystem path expansion and configuration-relative resolution.

use super::super::validation::{error, expand_user_path, required_string, resolve_path};
use crate::operation::OperationError;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn resolve_person_path(
    table: &toml::Table,
    field: &str,
    index: usize,
    base_dir: &Path,
) -> Result<PathBuf, OperationError> {
    let value = required_string(table, field, &format!("voce [[person]] {index}"))?;
    Ok(resolve_path(&value, base_dir))
}

pub(super) fn path_base(config_path: &Path) -> Result<PathBuf, OperationError> {
    let resolved = absolute_non_strict(config_path)?;
    let parent = resolved.parent().unwrap_or_else(|| Path::new("."));
    if parent.file_name().is_some_and(|name| name == "config") {
        Ok(parent.parent().unwrap_or(parent).to_path_buf())
    } else {
        Ok(parent.to_path_buf())
    }
}

pub(super) fn expand_path(path: &Path) -> PathBuf {
    path.to_str()
        .map(expand_user_path)
        .unwrap_or_else(|| path.to_path_buf())
}

pub(super) fn absolute_non_strict(path: &Path) -> Result<PathBuf, OperationError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|cause| error(format!("directory corrente non disponibile: {cause}")))?
            .join(path)
    };
    let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
    let filename = absolute.file_name();
    let canonical_parent = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    Ok(match filename {
        Some(name) => canonical_parent.join(name),
        None => canonical_parent,
    })
}
