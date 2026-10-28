//! Shared optional path validation for configuration sections.

use super::super::validation::{error, resolve_path};
use crate::operation::OperationError;
use std::path::{Path, PathBuf};
use toml::Table;

pub(in crate::config) fn optional_path(
    table: &Table,
    field: &str,
    context: &str,
    base_dir: &Path,
    default: Option<&Path>,
) -> Result<Option<PathBuf>, OperationError> {
    let Some(value) = table.get(field) else {
        return Ok(default.map(Path::to_path_buf));
    };
    let Some(value) = value.as_str() else {
        return Err(error(format!(
            "{context} campo {field} deve essere una stringa non vuota"
        )));
    };
    if value.trim().is_empty() {
        return Err(error(format!(
            "{context} campo {field} deve essere una stringa non vuota"
        )));
    }
    let path = resolve_path(value.trim(), base_dir);
    if !path.exists() {
        return Err(error(format!(
            "{context} campo {field} percorso inesistente: {}",
            path.display()
        )));
    }
    Ok(Some(path))
}
