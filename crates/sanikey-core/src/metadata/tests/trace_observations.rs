//! Contratti sintetici Trace per i parser delle osservazioni longitudinali.

use super::common::TestDirectory;
use crate::logging::{Component, Level, Session};
use crate::metadata::parse::{parse_observation_point, parse_observation_series};
use crate::metadata::{ObservationPoint, ObservationSeries};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn records(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("log Trace sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect()
}

fn expected_path(path: &Path) -> Value {
    json!({
        "kind": "data",
        "value": {"encoding": "utf8", "value": path.to_str().expect("path UTF-8")}
    })
}

fn expected_argument(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn expected_debug<T: std::fmt::Debug>(value: &T) -> Value {
    json!({
        "kind": "data",
        "value": {
            "format": "text_redacted_known_secret_assignments",
            "value": format!("{value:#?}")
        }
    })
}

fn assert_span(records: &[Value], function: &str, arguments: Value, result: Value) -> u64 {
    let mut entries = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function);
    let entry = entries
        .next()
        .unwrap_or_else(|| panic!("span assente: {function}"));
    assert!(entries.next().is_none(), "span duplicata: {function}");
    assert_eq!(entry["component"], "core");
    assert_eq!(
        entry["arguments"], arguments,
        "argomenti completi di {function}"
    );

    let call_id = entry["call_id"].as_u64().expect("call_id numerico");
    let mut exits = records.iter().filter(|record| {
        record["type"] == "function_exit"
            && record["function"] == function
            && record["call_id"] == call_id
    });
    let exit = exits
        .next()
        .unwrap_or_else(|| panic!("uscita assente: {function}#{call_id}"));
    assert!(
        exits.next().is_none(),
        "uscita duplicata: {function}#{call_id}"
    );
    assert_eq!(exit["outcome"], "returned");
    assert!(
        exit["duration_ms"].as_u64().is_some(),
        "durata {function}#{call_id}"
    );
    assert_eq!(exit["result"], result, "risultato completo di {function}");
    call_id
}

#[test]
fn observation_parsers_trace_complete_inputs_results_call_ids_and_durations() {
    let directory = TestDirectory::new();
    let path = directory.path().join("observations.toml");
    let series_table: toml::Table = toml::from_str(
        r#"
id = "weight-series"
name = "Peso"
value_type = "numeric"
unit = "kg"
description = "Serie sintetica"
warn_duplicate_same_day = false
synonyms = ["massa", "peso corporeo"]
"#,
    )
    .expect("tabella serie sintetica");
    let point_table: toml::Table = toml::from_str(
        r#"
id = "weight-point-1"
series_id = "weight-series"
observation_date = "2026-10-05"
source_type = "manual"
source_reference = "diario sintetico"
numeric_value = 75.5
systolic = 120.0
diastolic = 80.0
pulse = 65.0
note = "misura sintetica"
"#,
    )
    .expect("tabella punto sintetica");
    let expected_series = ObservationSeries {
        id: "weight-series".to_owned(),
        name: "Peso".to_owned(),
        value_type: "numeric".to_owned(),
        unit: Some("kg".to_owned()),
        description: Some("Serie sintetica".to_owned()),
        warn_duplicate_same_day: false,
        synonyms: vec!["massa".to_owned(), "peso corporeo".to_owned()],
        parameter_rule_id: None,
        parameter_rule_version: None,
        parameter_rule_digest: None,
        unit_variant: None,
    };
    let expected_point = ObservationPoint {
        id: "weight-point-1".to_owned(),
        series_id: "weight-series".to_owned(),
        observation_date: "2026-10-05".to_owned(),
        source_type: "manual".to_owned(),
        source_reference: "diario sintetico".to_owned(),
        numeric_value: Some(75.5),
        text_value: None,
        systolic: Some(120.0),
        diastolic: Some(80.0),
        pulse: Some(65.0),
        note: Some("misura sintetica".to_owned()),
        source_kind: "curated-observation".to_owned(),
        document_id: None,
        document_href: None,
        document_title: None,
        document_category: None,
        source_text_digest: None,
        original_line: None,
        line_number: None,
        page_number: None,
        character_start: None,
        character_end: None,
        matched_label: None,
        raw_value: None,
        parsed_value: None,
        raw_unit: None,
        normalized_unit: None,
        qualifier: None,
        rule_id: None,
        rule_version: None,
        rule_digest: None,
        reason_code: None,
    };

    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");
    let series = session.scoped(|| parse_observation_series(&series_table, &path, 2));
    assert_eq!(series, Ok(expected_series.clone()));
    let point = session.scoped(|| parse_observation_point(&point_table, &path, 4));
    assert_eq!(point, Ok(expected_point.clone()));
    session.finish(Component::Core).expect("chiusura sessione");

    let trace = records(&log_path);
    let series_arguments = json!([
        expected_argument(
            "raw_item",
            json!({"kind": "data", "value": {
                "description": {"type": "string", "value": "Serie sintetica"},
                "id": {"type": "string", "value": "weight-series"},
                "name": {"type": "string", "value": "Peso"},
                "synonyms": {"type": "array", "value": [
                    {"type": "string", "value": "massa"},
                    {"type": "string", "value": "peso corporeo"}
                ]},
                "unit": {"type": "string", "value": "kg"},
                "value_type": {"type": "string", "value": "numeric"},
                "warn_duplicate_same_day": {"type": "boolean", "value": false}
            }})
        ),
        expected_argument("path", expected_path(&path)),
        expected_argument("index", json!({"kind": "data", "value": 2}))
    ]);
    let point_arguments = json!([
        expected_argument(
            "raw_item",
            json!({"kind": "data", "value": {
                "diastolic": {"type": "float", "value": "80"},
                "id": {"type": "string", "value": "weight-point-1"},
                "note": {"type": "string", "value": "misura sintetica"},
                "numeric_value": {"type": "float", "value": "75.5"},
                "observation_date": {"type": "string", "value": "2026-10-05"},
                "pulse": {"type": "float", "value": "65"},
                "series_id": {"type": "string", "value": "weight-series"},
                "source_reference": {"type": "string", "value": "diario sintetico"},
                "source_type": {"type": "string", "value": "manual"},
                "systolic": {"type": "float", "value": "120"}
            }})
        ),
        expected_argument("path", expected_path(&path)),
        expected_argument("index", json!({"kind": "data", "value": 4}))
    ]);
    assert!(
        point_arguments[0]["content"]["value"]
            .get("text_value")
            .is_none(),
        "text_value è omesso dall'input sintetico"
    );
    assert_eq!(
        expected_point.text_value, None,
        "text_value omesso resta None"
    );
    assert_eq!(
        expected_series.parameter_rule_id, None,
        "la provenienza da regola resta omessa"
    );
    let series_id = assert_span(
        &trace,
        "metadata::parse_observation_series",
        series_arguments,
        expected_debug(&expected_series),
    );
    let point_id = assert_span(
        &trace,
        "metadata::parse_observation_point",
        point_arguments,
        expected_debug(&expected_point),
    );
    assert_ne!(series_id, point_id, "call_id distinti per gli span");
}
