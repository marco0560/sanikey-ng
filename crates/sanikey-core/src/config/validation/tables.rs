//! TOML table validation and configuration errors.

use super::super::trace::{
    trace_config_call, trace_optional_toml_value, trace_sensitive_text, trace_table_result,
    trace_toml_table, trace_toml_table_result_ref,
};
use super::optional_string;
use crate::operation::{ErrorCode, OperationError};
use toml::{Table, Value};

pub(in crate::config) fn error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    trace_config_call!("config::validation::error", [], paths [], fields ["message" => trace_sensitive_text(&message)], || {
        OperationError::new(ErrorCode::Configuration, message)
    })
}

pub(in crate::config) fn table_or_empty(
    value: Option<&Value>,
    context: &str,
) -> Result<Table, OperationError> {
    trace_config_call!("config::validation::table_or_empty", ["context" => &context], paths [], fields ["value" => trace_optional_toml_value(value)], result_trace trace_table_result, || {
    match value {
        None => Ok(Table::new()),
        Some(Value::Table(table)) => Ok(table.clone()),
        Some(_) => Err(error(format!("{context} deve essere una tabella"))),
    }
    })
}

pub(in crate::config) fn require_table<'a>(
    value: &'a Value,
    context: &str,
) -> Result<&'a Table, OperationError> {
    let trace = crate::logging::trace_current(
        crate::logging::Component::Core,
        "config::validation::require_table",
        || {
            Vec::from([
                crate::logging::TraceField::new(
                    "value",
                    crate::config::trace::trace_toml_value(value),
                ),
                crate::logging::TraceField::new(
                    "context",
                    crate::config::trace::trace_debug(&context),
                ),
            ])
        },
    );
    let result = value
        .as_table()
        .ok_or_else(|| error(format!("{context} deve essere una tabella")));
    if let Some(trace) = trace {
        let _ = trace.finish(trace_toml_table_result_ref(&result));
    }
    result
}

pub(in crate::config) fn reject_unknown(
    table: &Table,
    allowed: &[&str],
    context: &str,
) -> Result<(), OperationError> {
    trace_config_call!("config::validation::reject_unknown", ["allowed" => allowed, "context" => &context], paths [], fields ["table" => trace_toml_table(table)], || {
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
    })
}

pub(in crate::config) fn required_string(
    table: &Table,
    field: &str,
    context: &str,
) -> Result<String, OperationError> {
    trace_config_call!("config::validation::required_string", ["field" => &field, "context" => &context], paths [], fields ["table" => trace_toml_table(table)], || {
        optional_string(table, field, context, None)?.ok_or_else(|| {
            error(format!("{context} campo {field} deve essere una stringa non vuota"))
        })
    })
}
