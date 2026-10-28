//! Primitive value validation.

use super::error;
use crate::operation::OperationError;
use toml::{Table, Value};

pub(in crate::config) fn optional_string(
    table: &Table,
    field: &str,
    context: &str,
    default: Option<&str>,
) -> Result<Option<String>, OperationError> {
    match table.get(field) {
        None => Ok(default.map(str::to_owned)),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.trim().to_owned())),
        Some(Value::String(_))
        | Some(Value::Datetime(_))
        | Some(Value::Integer(_))
        | Some(Value::Float(_))
        | Some(Value::Boolean(_))
        | Some(Value::Array(_))
        | Some(Value::Table(_)) => Err(error(format!(
            "{context} campo {field} deve essere una stringa non vuota"
        ))),
    }
}

pub(in crate::config) fn optional_nullable_string(
    table: &Table,
    field: &str,
    context: &str,
) -> Result<Option<String>, OperationError> {
    match table.get(field) {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.trim().to_owned())),
        Some(Value::String(_)) => Err(error(format!(
            "{context} campo {field} deve essere una stringa non vuota"
        ))),
        Some(_) => Err(error(format!(
            "{context} campo {field} deve essere una stringa non vuota o nullo"
        ))),
    }
}

pub(in crate::config) fn optional_bool(
    table: &Table,
    field: &str,
    context: &str,
    default: bool,
) -> Result<bool, OperationError> {
    match table.get(field) {
        None => Ok(default),
        Some(Value::Boolean(value)) => Ok(*value),
        Some(_) => Err(error(format!(
            "{context} campo {field} deve essere booleano"
        ))),
    }
}

pub(in crate::config) fn optional_positive_integer(
    table: &Table,
    field: &str,
    context: &str,
    default: i64,
) -> Result<i64, OperationError> {
    match table.get(field) {
        None => Ok(default),
        Some(Value::Integer(value)) if *value > 0 => Ok(*value),
        Some(_) => Err(error(format!(
            "{context} campo {field} deve essere un intero positivo"
        ))),
    }
}

pub(in crate::config) fn optional_choice(
    table: &Table,
    field: &str,
    context: &str,
    choices: &[&str],
    default: &str,
) -> Result<String, OperationError> {
    let Some(value) = table.get(field) else {
        return Ok(default.to_owned());
    };
    let Some(choice) = value.as_str() else {
        return Err(error(format!(
            "{context} campo {field} deve essere uno tra: {}",
            choices.join(", ")
        )));
    };
    if choices.contains(&choice) {
        Ok(choice.to_owned())
    } else {
        let mut sorted = choices.to_vec();
        sorted.sort_unstable();
        Err(error(format!(
            "{context} campo {field} deve essere uno tra: {}",
            sorted.join(", ")
        )))
    }
}

pub(in crate::config) fn optional_color(
    table: &Table,
    field: &str,
    context: &str,
    default: &str,
) -> Result<String, OperationError> {
    let Some(value) = table.get(field) else {
        return Ok(default.to_owned());
    };
    let Some(color) = value.as_str() else {
        return Err(error(format!(
            "{context} campo {field} deve essere un colore #rrggbb"
        )));
    };
    let bytes = color.as_bytes();
    if bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        Ok(color.to_ascii_lowercase())
    } else {
        Err(error(format!(
            "{context} campo {field} deve essere un colore #rrggbb"
        )))
    }
}

pub(in crate::config) fn optional_short_string(
    table: &Table,
    field: &str,
    context: &str,
    default: &str,
) -> Result<String, OperationError> {
    let Some(value) = table.get(field) else {
        return Ok(default.to_owned());
    };
    let Some(string) = value.as_str() else {
        return Err(error(format!(
            "{context} campo {field} deve essere una stringa"
        )));
    };
    let rendered = string.trim();
    if rendered.chars().count() > 120 {
        return Err(error(format!(
            "{context} campo {field} deve essere al massimo 120 caratteri"
        )));
    }
    Ok(rendered.to_owned())
}

pub(in crate::config) fn optional_float(
    table: &Table,
    field: &str,
    context: &str,
    default: f64,
    minimum: f64,
    maximum: f64,
) -> Result<f64, OperationError> {
    let Some(value) = table.get(field) else {
        return Ok(default);
    };
    let number = match value {
        Value::Integer(integer) => *integer as f64,
        Value::Float(float) => *float,
        _ => {
            return Err(error(format!(
                "{context} campo {field} deve essere un numero"
            )));
        }
    };
    if !number.is_finite() || number < minimum || number > maximum {
        return Err(error(format!(
            "{context} campo {field} deve essere tra {minimum} e {maximum}"
        )));
    }
    Ok(number)
}

pub(in crate::config) fn optional_string_list(
    table: &Table,
    field: &str,
    context: &str,
) -> Result<Vec<String>, OperationError> {
    let Some(value) = table.get(field) else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err(error(format!(
            "{context} campo {field} deve essere una lista di stringhe"
        )));
    };
    values
        .iter()
        .map(|value| match value.as_str() {
            Some(string) if !string.trim().is_empty() => Ok(string.trim().to_owned()),
            _ => Err(error(format!(
                "{context} campo {field} deve contenere solo stringhe non vuote"
            ))),
        })
        .collect()
}
