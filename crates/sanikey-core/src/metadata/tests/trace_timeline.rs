//! Contratti sintetici Trace per i parser della timeline.

use super::common::TestDirectory;
use crate::logging::{Component, Level, Session};
use crate::metadata::TimelineEvent;
use crate::metadata::parse::{parse_document_tags, parse_event, parse_summary};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn records(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("log sintetico")
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
fn timeline_parsers_trace_complete_inputs_results_ids_durations_and_secret_omissions() {
    let directory = TestDirectory::new();
    let event_path = directory.path().join("events.toml");
    let tags_path = directory.path().join("document-tags.toml");
    let summary_path = directory.path().join("summary.toml");
    directory.write(
        "document-tags.toml",
        "[tags]\n\"doc-synthetic\" = [\"referto\", \"sintetico\"]\n",
    );
    directory.write(
        "summary.toml",
        "summary = \"SINTESI-SINTETICA; api_key = SYNTHETIC-SECRET-OUTPUT\"\n",
    );

    let event_table: toml::Table = toml::from_str(
        r#"
id = "event-synthetic"
title = "Evento sintetico"
start_date = "2026-01-02"
end_date = "2026-01-03"
source = "manuale"
links = ["doc-a", "doc-b"]
api_key = "SECRET-TIMELINE-SYNTHETIC"
unmodeled_note = "campo preservato solo nel trace"
"#,
    )
    .expect("tabella evento sintetica");
    let expected_event = Ok(TimelineEvent {
        id: "event-synthetic".to_owned(),
        title: "Evento sintetico".to_owned(),
        start_date: Some("2026-01-02".to_owned()),
        end_date: Some("2026-01-03".to_owned()),
        source: "manuale".to_owned(),
        links: vec!["doc-a".to_owned(), "doc-b".to_owned()],
    });

    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");
    let event = session.scoped(|| parse_event(&event_table, &event_path, 7));
    assert_eq!(event, expected_event);
    let tags = session.scoped(|| parse_document_tags(&tags_path));
    assert_eq!(
        tags.expect("tag sintetici validi").get("doc-synthetic"),
        Some(&vec!["referto".to_owned(), "sintetico".to_owned()])
    );
    let summary = session.scoped(|| parse_summary(&summary_path));
    let summary_value = "SINTESI-SINTETICA; api_key = SYNTHETIC-SECRET-OUTPUT";
    assert_eq!(
        summary.expect("sintesi sintetica valida").as_deref(),
        Some(summary_value),
        "il parser restituisce il testo originale"
    );
    session.finish(Component::Core).expect("chiusura sessione");

    let trace = records(&log_path);
    let event_inputs = json!([
        expected_argument(
            "raw_item",
            json!({"kind": "data", "value": {
                    "api_key": {"kind": "secret_omitted"},
                    "end_date": {"type": "string", "value": "2026-01-03"},
                    "id": {"type": "string", "value": "event-synthetic"},
                    "links": {"type": "array", "value": [
                        {"type": "string", "value": "doc-a"},
                        {"type": "string", "value": "doc-b"}
                    ]},
                    "source": {"type": "string", "value": "manuale"},
                    "start_date": {"type": "string", "value": "2026-01-02"},
                    "title": {"type": "string", "value": "Evento sintetico"},
                    "unmodeled_note": {"type": "string", "value": "campo preservato solo nel trace"}
                }
            })
        ),
        expected_argument("path", expected_path(&event_path)),
        expected_argument("index", json!({"kind": "data", "value": 7}))
    ]);
    let event_result = expected_debug(expected_event.as_ref().expect("evento atteso"));
    let expected_tags = indexmap::IndexMap::from([(
        "doc-synthetic".to_owned(),
        vec!["referto".to_owned(), "sintetico".to_owned()],
    )]);
    let tags_result = expected_debug(&expected_tags);
    let summary_result = json!({
        "kind": "data",
        "value": {
            "format": "text_redacted_known_secret_assignments",
            "value": "Some(\n    \"SINTESI-SINTETICA; api_key = [secret_omitted]\",\n)"
        }
    });

    let event_id = assert_span(&trace, "metadata::parse_event", event_inputs, event_result);
    let tags_id = assert_span(
        &trace,
        "metadata::parse_document_tags",
        json!([expected_argument("path", expected_path(&tags_path))]),
        tags_result,
    );
    let summary_id = assert_span(
        &trace,
        "metadata::parse_summary",
        json!([expected_argument("path", expected_path(&summary_path))]),
        summary_result,
    );
    assert_ne!(event_id, tags_id);
    assert_ne!(event_id, summary_id);
    assert_ne!(tags_id, summary_id);
    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(!log.contains("SECRET-TIMELINE-SYNTHETIC"));
    assert!(!log.contains("SYNTHETIC-SECRET-OUTPUT"));
    assert!(log.contains("api_key = [secret_omitted]"));
}
