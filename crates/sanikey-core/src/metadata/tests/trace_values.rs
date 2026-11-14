//! Verifiche complete delle span per i convertitori primitivi TOML.

use super::common::TestDirectory;
use crate::logging::{Component, Level, Session};
use crate::metadata::value::{
    optional_bool, optional_float, optional_string, require_table, required_string, string_tuple,
    value_error,
};
use serde_json::{Value, json};
use std::fmt::Debug;
use std::path::Path;

fn expected_data(value: Value) -> Value {
    json!({"kind": "data", "value": value})
}

fn expected_debug(value: &impl Debug, secret: Option<&str>) -> Value {
    let mut rendered = format!("{value:#?}");
    if let Some(secret) = secret {
        rendered = rendered.replace(secret, "[secret_omitted]");
    }
    expected_data(json!({
        "format": "text_redacted_known_secret_assignments",
        "value": rendered,
    }))
}

fn expected_sensitive_text(value: &str) -> Value {
    expected_data(json!({
        "format": "text_redacted_known_secret_assignments",
        "value": value,
    }))
}

fn expected_path(path: &Path) -> Value {
    expected_data(json!({"encoding": "utf8", "value": path.to_str().unwrap()}))
}

fn expected_argument(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn assert_trace_spans(records: &[Value], expected: Vec<(&str, Value, Value, Option<&str>)>) {
    let enters = records
        .iter()
        .filter(|record| record["type"] == "function_enter")
        .collect::<Vec<_>>();
    let exits = records
        .iter()
        .filter(|record| record["type"] == "function_exit")
        .collect::<Vec<_>>();
    assert_eq!(
        enters.len(),
        expected.len(),
        "numero di ingressi Trace: {:?}",
        enters
            .iter()
            .map(|entry| &entry["function"])
            .collect::<Vec<_>>()
    );
    assert_eq!(exits.len(), expected.len(), "numero di uscite Trace");

    let mut unmatched = enters;
    let mut call_ids = Vec::new();
    for (function, arguments, result, parent_function) in expected {
        let index = unmatched
            .iter()
            .position(|entry| entry["function"] == function && entry["arguments"] == arguments)
            .unwrap_or_else(|| {
                panic!(
                    "ingresso atteso assente per {function}: {arguments}; presenti: {:?}",
                    unmatched
                        .iter()
                        .filter(|entry| entry["function"] == function)
                        .map(|entry| &entry["arguments"])
                        .collect::<Vec<_>>()
                )
            });
        let entry = unmatched.remove(index);
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        assert!(!call_ids.contains(&call_id), "call_id univoco");
        call_ids.push(call_id);

        let correlated = exits
            .iter()
            .filter(|exit| exit["call_id"] == call_id)
            .collect::<Vec<_>>();
        assert_eq!(
            correlated.len(),
            1,
            "una uscita correlata per {function}#{call_id}"
        );
        assert_eq!(correlated[0]["function"], function);
        assert_eq!(correlated[0]["outcome"], "returned");
        assert_eq!(
            correlated[0]["result"], result,
            "risultato di {function}#{call_id}"
        );
        assert!(
            correlated[0]["duration_ms"].as_u64().is_some(),
            "durata numerica per {function}#{call_id}"
        );

        match parent_function {
            Some(parent_function) => {
                let parent = records
                    .iter()
                    .find(|record| {
                        record["type"] == "function_enter" && record["function"] == parent_function
                    })
                    .expect("ingresso del genitore");
                assert_eq!(entry["parent_call_id"], parent["call_id"]);
            }
            None => assert!(
                entry["parent_call_id"].is_null(),
                "span radice senza genitore"
            ),
        }
    }
    assert!(unmatched.is_empty());
    assert!(
        records
            .iter()
            .all(|record| record["result"]["reason"] != "unfinished_call"),
        "nessuna span incompleta"
    );
}

#[test]
fn primitive_toml_helpers_trace_complete_inputs_outputs_and_secret_omission() {
    let directory = TestDirectory::new();
    let log_directory = directory.path().join("logs");
    let metadata_path = Path::new("/synthetic/metadata.toml");
    let secret = "SYNTHETIC-SECRET-ONLY";
    let table_value: toml::Value = toml::from_str(&format!(
        "title = \"Synthetic title\"\napi_key = \"{secret}\"\n"
    ))
    .expect("tabella TOML sintetica");
    let expected_table = toml::Table::from_iter([
        (
            "title".to_owned(),
            toml::Value::String("Synthetic title".to_owned()),
        ),
        ("api_key".to_owned(), toml::Value::String(secret.to_owned())),
    ]);
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");

    let mut expected = Vec::new();
    let mut generated_error = None;
    session.scoped(|| {
        let table_result = require_table(&table_value, metadata_path, 3);
        assert_eq!(table_result.expect("tabella valida"), &expected_table);
        let table_arguments = json!([
            expected_argument(
                "raw_value",
                expected_data(json!({
                    "api_key": {"kind": "secret_omitted"},
                    "title": {"type": "string", "value": "Synthetic title"},
                })),
            ),
            expected_argument("path", expected_path(metadata_path)),
            expected_argument("index", expected_data(json!(3))),
        ]);
        expected.push((
            "metadata::require_table",
            table_arguments,
            expected_data(json!(true)),
            None,
        ));

        let item = toml::from_str::<toml::Table>(
            "name = \"  Synthetic patient  \"\nnickname = \"  Patient nickname  \"\ncount = 120\nenabled = true\ntags = [\" alpha \", \"\", \"beta\"]\n",
        )
        .expect("elemento TOML sintetico");

        let name = required_string(&item, "name", metadata_path, 5).expect("nome sintetico");
        assert_eq!(name, "Synthetic patient");
        expected.push((
            "metadata::required_string",
            json!([
                expected_argument("field", expected_data(json!("name"))),
                expected_argument(
                    "value",
                    expected_data(json!({"name": {"type": "string", "value": "  Synthetic patient  "}})),
                ),
                expected_argument("path", expected_path(metadata_path)),
                expected_argument("index", expected_data(json!(5))),
            ]),
            expected_debug(&"Synthetic patient".to_owned(), None),
            None,
        ));

        let nickname = optional_string(&item, "nickname").expect("nickname sintetico");
        assert_eq!(nickname.as_deref(), Some("Patient nickname"));
        expected.push((
            "metadata::optional_string",
            json!([
                expected_argument("field", expected_data(json!("nickname"))),
                expected_argument(
                    "value",
                    expected_data(json!({"nickname": {"type": "string", "value": "  Patient nickname  "}})),
                ),
            ]),
            expected_debug(&Some("Patient nickname".to_owned()), None),
            None,
        ));

        let enabled = optional_bool(&item, "enabled", false, metadata_path, 8)
            .expect("booleano sintetico");
        assert!(enabled);
        expected.push((
            "metadata::optional_bool",
            json!([
                expected_argument("field", expected_data(json!("enabled"))),
                expected_argument("default", expected_data(json!(false))),
                expected_argument(
                    "value",
                    expected_data(json!({"enabled": {"type": "boolean", "value": true}})),
                ),
                expected_argument("path", expected_path(metadata_path)),
                expected_argument("index", expected_data(json!(8))),
            ]),
            expected_debug(&true, None),
            None,
        ));

        let count = optional_float(&item, "count", metadata_path, 9)
            .expect("numero sintetico");
        assert_eq!(count, Some(120.0));
        expected.push((
            "metadata::optional_float",
            json!([
                expected_argument("field", expected_data(json!("count"))),
                expected_argument(
                    "value",
                    expected_data(json!({"count": {"type": "integer", "value": 120}})),
                ),
                expected_argument("path", expected_path(metadata_path)),
                expected_argument("index", expected_data(json!(9))),
            ]),
            expected_debug(&Some(120.0), None),
            None,
        ));

        let tags_value = item.get("tags").expect("tags sintetici");
        let tags = string_tuple(Some(tags_value), metadata_path, "tags", 11)
            .expect("tuple sintetica");
        assert_eq!(tags, ["alpha", "beta"]);
        expected.push((
            "metadata::string_tuple",
            json!([
                expected_argument("field", expected_data(json!("tags"))),
                expected_argument(
                    "value",
                    expected_data(json!({"tags": {"type": "array", "value": [
                        {"type": "string", "value": " alpha "},
                        {"type": "string", "value": ""},
                        {"type": "string", "value": "beta"},
                    ]}})),
                ),
                expected_argument("path", expected_path(metadata_path)),
                expected_argument("index", expected_data(json!(11))),
            ]),
            expected_debug(&vec!["alpha".to_owned(), "beta".to_owned()], None),
            None,
        ));

        generated_error = Some(value_error("errore sintetico del parser"));
        let message = expected_sensitive_text("errore sintetico del parser");
        expected.push((
            "metadata::value_error",
            json!([]),
            expected_data(json!([
                expected_argument("code", expected_data(json!("configuration"))),
                expected_argument("message", message.clone()),
            ])),
            None,
        ));
        expected.push((
            "operation::OperationError::new",
            json!([
                expected_argument("code", expected_data(json!("configuration"))),
                expected_argument("message", message.clone()),
            ]),
            expected_data(json!({"code": "configuration", "message": message})),
            Some("metadata::value_error"),
        ));
        expected.push((
            "operation::OperationError::message",
            json!([expected_argument(
                "self",
                expected_data(json!({
                    "code": "configuration",
                    "message": expected_sensitive_text("errore sintetico del parser"),
                })),
            )]),
            expected_sensitive_text("errore sintetico del parser"),
            Some("metadata::value_error"),
        ));
    });

    let generated_error = generated_error.expect("errore applicativo sintetico");
    session.finish(Component::Core).expect("chiusura sessione");
    assert_eq!(
        generated_error.code(),
        crate::operation::ErrorCode::Configuration
    );
    assert_eq!(generated_error.message(), "errore sintetico del parser");
    let contents = std::fs::read_to_string(log_path).expect("log Trace sintetico");
    let records = contents
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("record JSON valido"))
        .collect::<Vec<_>>();
    assert_trace_spans(&records, expected);
    assert!(!contents.contains("SYNTHETIC-SECRET-ONLY"));
    assert!(contents.contains("secret_omitted"));
    assert!(!contents.contains("\"kind\":\"omitted\""));
}
