//! Lettura TOML e array di record dai file curati.

use super::value::{require_table, value_error};
use crate::operation::OperationError;
use std::fs;
use std::path::Path;
use toml::{Table, Value};

pub(super) fn load_toml(path: &Path) -> Result<Table, OperationError> {
    if !path.exists() {
        return Ok(Table::new());
    }
    let source = fs::read_to_string(path).map_err(|cause| {
        value_error(format!(
            "impossibile leggere metadati {}: {cause}",
            path.display()
        ))
    })?;
    let value: Value = toml::from_str(&source)
        .map_err(|cause| value_error(format!("TOML non valido in {}: {cause}", path.display())))?;
    value.as_table().cloned().ok_or_else(|| {
        value_error(format!(
            "file metadati deve contenere una tabella: {}",
            path.display()
        ))
    })
}

pub(super) fn load_items<T>(
    path: &Path,
    key: &str,
    parser: fn(&Table, &Path, usize) -> Result<T, OperationError>,
) -> Result<Vec<T>, OperationError> {
    let table = load_toml(path)?;
    let Some(raw_items) = table.get(key) else {
        return Ok(Vec::new());
    };
    let Some(items) = raw_items.as_array() else {
        return Err(value_error(format!(
            "{}: {key} deve essere un array di tabelle",
            path.display()
        )));
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let item = require_table(item, path, index)?;
            parser(item, path, index)
        })
        .collect()
}
