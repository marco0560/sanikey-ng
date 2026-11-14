//! Tracing helpers shared by configuration parsing and profile operations.

use super::{AccountsConfig, AccountsProfile, PersonConfig, SearchConfig, UiConfig};
use crate::logging::TraceValue;
pub(crate) use crate::logging::{
    trace_optional_toml_value, trace_sensitive_text, trace_toml_table, trace_toml_value,
};
use crate::operation::OperationError;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

macro_rules! trace_config_call {
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], fields [$($field_name:literal => $field_value:expr),* $(,)?], result_trace $result_trace:expr, $body:expr) => {{
        let trace = $crate::logging::trace_current($crate::logging::Component::Core, $function, || {
            Vec::from([
                $($crate::logging::TraceField::new($name, $crate::config::trace::trace_debug($value)),)*
                $($crate::logging::TraceField::new($path_name, $crate::logging::TraceValue::path($path)),)*
                $($crate::logging::TraceField::new($field_name, $field_value),)*
            ])
        });
        let result = $crate::config::trace::run_config_body($body);
        if let Some(trace) = trace {
            let _ = trace.finish(($result_trace)(&result));
        }
        result
    }};
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], fields [$($field_name:literal => $field_value:expr),* $(,)?], result_paths $result_paths:expr, $body:expr) => {{
        let trace = $crate::logging::trace_current($crate::logging::Component::Core, $function, || {
            Vec::from([
                $($crate::logging::TraceField::new($name, $crate::config::trace::trace_debug($value)),)*
                $($crate::logging::TraceField::new($path_name, $crate::logging::TraceValue::path($path)),)*
                $($crate::logging::TraceField::new($field_name, $field_value),)*
            ])
        });
        let result = $crate::config::trace::run_config_body($body);
        if let Some(trace) = trace {
            let _ = trace.finish($crate::logging::TraceValue::Data(serde_json::json!({
                "format":"rust_debug",
                "value":$crate::logging::redact_secret_assignments(&format!("{result:#?}")),
                "native_paths":$result_paths(&result).to_json(),
            })));
        }
        result
    }};
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], fields [$($field_name:literal => $field_value:expr),* $(,)?], $body:expr) => {
        trace_config_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [$($field_name => $field_value),*], result_paths $crate::config::trace::trace_no_paths, $body)
    };
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], result_paths $result_paths:expr, $body:expr) => {
        trace_config_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [], result_paths $result_paths, $body)
    };
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], paths [$($path_name:literal => $path:expr),* $(,)?], $body:expr) => {
        trace_config_call!($function, [$($name => $value),*], paths [$($path_name => $path),*], fields [], result_paths $crate::config::trace::trace_no_paths, $body)
    };
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], result_paths $result_paths:expr, $body:expr) => {
        trace_config_call!($function, [$($name => $value),*], paths [], fields [], result_paths $result_paths, $body)
    };
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], $body:expr) => {
        trace_config_call!($function, [$($name => $value),*], paths [], fields [], result_paths $crate::config::trace::trace_no_paths, $body)
    };
}

pub(crate) use trace_config_call;

pub(crate) fn run_config_body<R>(body: impl FnOnce() -> R) -> R {
    body()
}

pub(crate) fn trace_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> TraceValue {
    TraceValue::Data(json!({
        "format":"rust_debug",
        "value":crate::logging::redact_secret_assignments(&format!("{value:#?}")),
    }))
}

pub(crate) fn trace_no_paths<T: ?Sized>(_: &T) -> TraceValue {
    TraceValue::Data(json!([]))
}

pub(crate) fn trace_table_result(result: &Result<toml::Table, OperationError>) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|table| crate::logging::trace_toml_table(table).to_json())
            .unwrap_or_else(|error| {
                json!({"state":"error","code":error.code().as_str(),"message":crate::logging::trace_sensitive_text(error.message()).to_json()})
            }),
    )
}

pub(crate) fn trace_toml_value_result_ref(
    result: &Result<&toml::Value, OperationError>,
) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|value| crate::logging::trace_toml_value(value).to_json())
            .unwrap_or_else(|error| {
                json!({"state":"error","code":error.code().as_str(),"message":crate::logging::trace_sensitive_text(error.message()).to_json()})
            }),
    )
}

pub(crate) fn trace_toml_table_result_ref(
    result: &Result<&toml::Table, OperationError>,
) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|table| crate::logging::trace_toml_table(table).to_json())
            .unwrap_or_else(|error| {
                json!({"state":"error","code":error.code().as_str(),"message":crate::logging::trace_sensitive_text(error.message()).to_json()})
            }),
    )
}

pub(crate) fn trace_person_paths(person: &PersonConfig) -> TraceValue {
    TraceValue::Data(json!([
        {"name":"source_documents","path":TraceValue::path(&person.source_documents).to_json()},
        {"name":"metadata_directory","path":TraceValue::path(&person.metadata_directory).to_json()},
        {"name":"local_build","path":TraceValue::path(&person.local_build).to_json()},
        {"name":"ui.background_image","path":person.ui.background_image.as_deref().map(TraceValue::path).map(|path|path.to_json())},
        {"name":"search.dictionary","path":person.search.dictionary.as_deref().map(TraceValue::path).map(|path|path.to_json())}
    ]))
}

pub(crate) fn trace_people_paths(people: &[PersonConfig]) -> TraceValue {
    TraceValue::Data(Value::Array(people.iter().enumerate().map(|(index, person)|json!({"index":index,"paths":trace_person_paths(person).to_json()})).collect()))
}

pub(crate) fn trace_accounts_paths(config: &AccountsConfig) -> TraceValue {
    TraceValue::Data(json!({
        "config_path":TraceValue::path(&config.path).to_json(),
        "people":config.people.iter().enumerate().map(|(index, person)| json!({"index":index,"paths":trace_person_paths(person).to_json()})).collect::<Vec<_>>(),
        "ui.background_image":config.ui.background_image.as_deref().map(TraceValue::path).map(|path|path.to_json()),
        "search.dictionary":config.search.dictionary.as_deref().map(TraceValue::path).map(|path|path.to_json())
    }))
}

pub(crate) fn trace_ui_paths(config: &UiConfig) -> TraceValue {
    TraceValue::Data(
        json!([{"name":"background_image","path":config.background_image.as_deref().map(TraceValue::path).map(|path|path.to_json())}]),
    )
}

pub(crate) fn trace_optional_ui_paths(config: Option<&UiConfig>) -> TraceValue {
    config
        .map(trace_ui_paths)
        .unwrap_or(TraceValue::Data(Value::Null))
}

pub(crate) fn trace_search_paths(config: &SearchConfig) -> TraceValue {
    TraceValue::Data(
        json!([{"name":"dictionary","path":config.dictionary.as_deref().map(TraceValue::path).map(|path|path.to_json())}]),
    )
}

pub(crate) fn trace_path_result(result: &Result<PathBuf, OperationError>) -> TraceValue {
    TraceValue::Data(
        json!({"state":if result.is_ok(){"complete"}else{"error"},"path":result.as_ref().ok().map(|path|TraceValue::path(path).to_json())}),
    )
}

pub(crate) fn trace_optional_path_result(
    result: &Result<Option<PathBuf>, OperationError>,
) -> TraceValue {
    TraceValue::Data(
        json!({"state":if result.is_err(){"error"}else{"complete"},"path":result.as_ref().ok().and_then(|path|path.as_deref()).map(|path|TraceValue::path(path).to_json())}),
    )
}

pub(crate) fn trace_accounts_result(result: &Result<AccountsConfig, OperationError>) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|config| trace_accounts_paths(config).to_json())
            .unwrap_or_else(|_| json!({"state":"error","paths":[]})),
    )
}

pub(crate) fn trace_person_result(result: &Result<PersonConfig, OperationError>) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|person| trace_person_paths(person).to_json())
            .unwrap_or_else(|_| json!({"state":"error","paths":[]})),
    )
}

pub(crate) fn trace_ui_result(result: &Result<UiConfig, OperationError>) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|config| trace_ui_paths(config).to_json())
            .unwrap_or_else(|_| json!({"state":"error","paths":[]})),
    )
}

pub(crate) fn trace_search_result(result: &Result<SearchConfig, OperationError>) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|config| trace_search_paths(config).to_json())
            .unwrap_or_else(|_| json!({"state":"error","paths":[]})),
    )
}

pub(crate) fn trace_profile_result_paths(
    result: &Result<AccountsProfile, OperationError>,
) -> TraceValue {
    TraceValue::Data(
        result
            .as_ref()
            .map(|profile| {
                json!({
                    "source_path": TraceValue::path(profile.source_path()).to_json(),
                    "base_directory": TraceValue::path(profile.base_directory()).to_json(),
                    "accounts": trace_accounts_paths(profile.accounts()).to_json(),
                })
            })
            .unwrap_or_else(|_| json!({"state":"error","paths":[]})),
    )
}

pub(crate) fn trace_path(path: &Path) -> TraceValue {
    TraceValue::path(path)
}

pub(crate) fn trace_optional_path(path: Option<&Path>) -> TraceValue {
    TraceValue::Data(
        path.map(|path| TraceValue::path(path).to_json())
            .unwrap_or(Value::Null),
    )
}
