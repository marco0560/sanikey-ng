//! Parsing helpers for the account configuration.

mod load;
mod paths;
mod person;

pub use load::{default_accounts_path, load_accounts, parse_accounts_data};
pub(super) use paths::{absolute_non_strict, path_base};

use crate::logging::{Component, TraceField, trace_current};
use crate::operation::OperationError;
use toml::{Table, Value};

pub(super) fn root_section<'a>(global: &'a Table, key: &str) -> Result<&'a Value, OperationError> {
    let trace = trace_current(Component::Core, "config::root_section", || {
        Vec::from([
            TraceField::new("global", crate::config::trace::trace_toml_table(global)),
            TraceField::new("key", crate::config::trace::trace_debug(&key)),
        ])
    });
    static EMPTY_TABLE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let result = if let Some(value) = global.get(key) {
        Ok(value)
    } else {
        Ok(EMPTY_TABLE.get_or_init(|| Value::Table(Table::new())))
    };
    if let Some(trace) = trace {
        let _ = trace.finish(crate::config::trace::trace_toml_value_result_ref(&result));
    }
    result
}
