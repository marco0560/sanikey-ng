//! Validazione dei tipi primitivi nei metadati TOML.

use crate::operation::{ErrorCode, OperationError};
use std::path::Path;
use toml::{Table, Value};

pub(super) fn require_table<'a>(
    value: &'a Value,
    path: &Path,
    index: usize,
) -> Result<&'a Table, OperationError> {
    value.as_table().ok_or_else(|| {
        value_error(format!(
            "{}: elemento {index} deve essere una tabella",
            path.display()
        ))
    })
}

pub(super) fn required_string(
    item: &Table,
    field: &str,
    path: &Path,
    index: usize,
) -> Result<String, OperationError> {
    let value = item.get(field).and_then(Value::as_str).map(str::trim);
    match value {
        Some(value) if !value.is_empty() => Ok(value.to_owned()),
        _ => Err(value_error(format!(
            "{}: elemento {index} campo {field} deve essere una stringa non vuota",
            path.display()
        ))),
    }
}

pub(super) fn optional_string(item: &Table, field: &str) -> Result<Option<String>, OperationError> {
    match item.get(field) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.trim().to_owned())),
        Some(_) => Err(value_error(format!(
            "campo {field} deve essere una stringa"
        ))),
    }
}

pub(super) fn optional_bool(
    item: &Table,
    field: &str,
    default: bool,
    path: &Path,
    index: usize,
) -> Result<bool, OperationError> {
    match item.get(field) {
        None => Ok(default),
        Some(Value::Boolean(value)) => Ok(*value),
        Some(_) => Err(value_error(format!(
            "{}: elemento {index} campo {field} deve essere booleano",
            path.display()
        ))),
    }
}

pub(super) fn optional_float(
    item: &Table,
    field: &str,
    path: &Path,
    index: usize,
) -> Result<Option<f64>, OperationError> {
    match item.get(field) {
        None => Ok(None),
        Some(Value::Integer(value)) => Ok(Some(*value as f64)),
        Some(Value::Float(value)) => Ok(Some(*value)),
        Some(_) => Err(value_error(format!(
            "{}: elemento {index} campo {field} deve essere numerico",
            path.display()
        ))),
    }
}

pub(super) fn string_tuple(
    value: Option<&Value>,
    path: &Path,
    field: &str,
    index: usize,
) -> Result<Vec<String>, OperationError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err(value_error(format!(
            "{}: elemento {index} campo {field} deve essere un array di stringhe",
            path.display()
        )));
    };
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        let Some(value) = value.as_str() else {
            return Err(value_error(format!(
                "{}: elemento {index} campo {field} deve contenere solo stringhe",
                path.display()
            )));
        };
        let value = value.trim();
        if !value.is_empty() {
            result.push(value.to_owned());
        }
    }
    Ok(result)
}

pub(super) fn value_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}
