//! Lettura TOML e array di record dai file curati.

use super::value::{require_table, value_error};
use super::{trace_metadata_call, trace_metadata_result};
use crate::logging::{
    TraceField, TraceOmission, TraceValue, trace_sensitive_text, trace_toml_table,
};
use crate::operation::OperationError;
use std::fs;
use std::path::Path;
use toml::{Table, Value};

pub(super) fn load_toml(path: &Path) -> Result<Table, OperationError> {
    trace_metadata_call(
        "metadata::load_toml",
        || vec![TraceField::new("path", TraceValue::path(path))],
        || {
            if !path.exists() {
                return Ok(Table::new());
            }
            let source = fs::read_to_string(path).map_err(|cause| {
                value_error(format!(
                    "impossibile leggere metadati {}: {cause}",
                    path.display()
                ))
            })?;
            let value: Value = toml::from_str(&source).map_err(|cause| {
                value_error(format!("TOML non valido in {}: {cause}", path.display()))
            })?;
            value.as_table().cloned().ok_or_else(|| {
                value_error(format!(
                    "file metadati deve contenere una tabella: {}",
                    path.display()
                ))
            })
        },
        |result| match result {
            Ok(table) => trace_toml_table(table),
            Err(error) => TraceValue::record(&[
                TraceField::new("code", TraceValue::data(error.code().as_str())),
                TraceField::new("message", trace_sensitive_text(error.message())),
            ]),
        },
    )
}

pub(super) fn load_items<T: std::fmt::Debug>(
    path: &Path,
    key: &str,
    parser: fn(&Table, &Path, usize) -> Result<T, OperationError>,
) -> Result<Vec<T>, OperationError> {
    trace_metadata_call(
        "metadata::load_items",
        || {
            vec![
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("key", TraceValue::data(key)),
                TraceField::new(
                    "parser",
                    TraceValue::Omitted(TraceOmission::UnsupportedType),
                ),
            ]
        },
        || {
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
        },
        trace_metadata_result,
    )
}
