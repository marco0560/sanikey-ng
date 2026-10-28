//! Parsing helpers for the account configuration.

mod load;
mod paths;
mod person;

pub use load::{default_accounts_path, load_accounts, parse_accounts_data};
pub(super) use paths::{absolute_non_strict, path_base};

use crate::operation::OperationError;
use toml::{Table, Value};

pub(super) fn root_section<'a>(global: &'a Table, key: &str) -> Result<&'a Value, OperationError> {
    static EMPTY_TABLE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    if let Some(value) = global.get(key) {
        return Ok(value);
    }
    Ok(EMPTY_TABLE.get_or_init(|| Value::Table(Table::new())))
}
