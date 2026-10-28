//! TOML table validation and configuration errors.

use super::optional_string;
use crate::operation::{ErrorCode, OperationError};
use toml::{Table, Value};

pub(in crate::config) fn error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}

pub(in crate::config) fn table_or_empty(
    value: Option<&Value>,
    context: &str,
) -> Result<Table, OperationError> {
    match value {
        None => Ok(Table::new()),
        Some(Value::Table(table)) => Ok(table.clone()),
        Some(_) => Err(error(format!("{context} deve essere una tabella"))),
    }
}

pub(in crate::config) fn require_table<'a>(
    value: &'a Value,
    context: &str,
) -> Result<&'a Table, OperationError> {
    value
        .as_table()
        .ok_or_else(|| error(format!("{context} deve essere una tabella")))
}

pub(in crate::config) fn reject_unknown(
    table: &Table,
    allowed: &[&str],
    context: &str,
) -> Result<(), OperationError> {
    let mut unknown: Vec<_> = table
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect();
    unknown.sort();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(error(format!(
            "{context} campi sconosciuti: {}",
            unknown.join(", ")
        )))
    }
}

pub(in crate::config) fn required_string(
    table: &Table,
    field: &str,
    context: &str,
) -> Result<String, OperationError> {
    optional_string(table, field, context, None)?.ok_or_else(|| {
        error(format!(
            "{context} campo {field} deve essere una stringa non vuota"
        ))
    })
}
