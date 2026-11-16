//! Verifica le span Trace della scoperta sintetica di candidati ai parametri.

use sanikey_core::logging::{Component, Level, Session};
use sanikey_core::metadata::DocumentRecord;
use sanikey_core::parameter_slices::{
    CandidateGroup, DiscoveryResult, DiscoverySettings, ParameterCandidate,
    discover_configured_candidates,
};
use sanikey_core::pdf::{ExtractedText, TextPageSpan};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Creates and exposes the synthetic directory used by the test.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-parameter-discovery-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory sintetica");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Removes the synthetic directory when the fixture is dropped.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn trace_path(path: &Path) -> Value {
    json!({
        "kind": "data",
        "value": {"encoding": "utf8", "value": path.to_str().expect("path UTF-8")}
    })
}

fn trace_debug(value: &str) -> Value {
    json!({"kind":"data", "value":{"format":"rust_debug", "value":value}})
}

fn argument(name: &str, content: Value) -> Value {
    json!({"name":name, "content":content})
}

fn span(records: &[Value], function: &str, expected_args: Value, expected_result: Value) -> u64 {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), 1, "una sola entrata per {function}");
    let entry = entries[0];
    assert_eq!(entry["component"], "core");
    assert_eq!(entry["arguments"], expected_args, "argomenti {function}");
    let call_id = entry["call_id"].as_u64().expect("call_id numerico");
    let exits: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_exit"
                && record["function"] == function
                && record["call_id"] == call_id
        })
        .collect();
    assert_eq!(exits.len(), 1, "una sola uscita per {function}#{call_id}");
    assert_eq!(exits[0]["outcome"], "returned");
    assert!(exits[0]["duration_ms"].as_u64().is_some());
    assert_eq!(exits[0]["result"], expected_result, "risultato {function}");
    call_id
}

fn trace_records(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("log Trace sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect()
}

#[test]
fn configured_parameter_discovery_traces_inputs_results_ids_and_durations() {
    let directory = TestDirectory::new();
    let document = DocumentRecord {
        document_id: "doc-1".to_owned(),
        patient_id: "synthetic-person".to_owned(),
        path: PathBuf::from("/synthetic/doc-1.txt"),
        title: "Synthetic lab report".to_owned(),
        category: "laboratory".to_owned(),
        kind: "text".to_owned(),
        sha256: "synthetic-document-digest".to_owned(),
        date: Some("2026-11-01".to_owned()),
        series: None,
        tags: vec!["laboratory".to_owned()],
        origin: "source".to_owned(),
        container_id: None,
        internal_path: None,
    };
    let extraction = ExtractedText {
        document_id: document.document_id.clone(),
        text: "Hb: 13.7 g/dL\n".to_owned(),
        warnings: Vec::new(),
        page_spans: vec![TextPageSpan {
            page_number: 2,
            character_start: 0,
            character_end: 13,
        }],
    };
    let settings = DiscoverySettings {
        min_label_length: 2,
        max_label_length: 80,
        max_label_words: 8,
        min_occurrences: 2,
        min_distinct_documents: 2,
        min_distinct_dates: 1,
        excluded_labels: Vec::new(),
    };
    let documents = [document.clone()];
    let extracted = [extraction.clone()];
    let accepted_labels = ["Hb".to_owned()];
    let hrefs = std::collections::HashMap::from([(
        document.document_id.clone(),
        "../documents/doc-1.txt".to_owned(),
    )]);
    let expected_candidate = ParameterCandidate {
        stable_id: "631803c17f021ad6f4ce67ecf8f9f6633ca9569e6344201bf9f02f02a835b494".to_owned(),
        document_id: "doc-1".to_owned(),
        document_date: Some("2026-11-01".to_owned()),
        document_href: Some("../documents/doc-1.txt".to_owned()),
        document_title: "Synthetic lab report".to_owned(),
        document_name: "doc-1.txt".to_owned(),
        document_category: "laboratory".to_owned(),
        source_text_digest: "183037b0941bb61c04b76efb0aa38a2cf6d4279b9bee3c46472891a67e57d34c"
            .to_owned(),
        line_number: 1,
        character_start: 0,
        character_end: 13,
        original_line: "Hb: 13.7 g/dL".to_owned(),
        normalized_label: "hb".to_owned(),
        raw_value: "13.7".to_owned(),
        parsed_value: Some(13.7),
        number_format: Some("decimal-point".to_owned()),
        qualifier: None,
        raw_unit: Some("g/dL".to_owned()),
        prefix_context: String::new(),
        suffix_context: String::new(),
        reason_code: None,
        page_number: Some(2),
        context: "Hb: 13.7 g/dL".to_owned(),
        page_label: "2".to_owned(),
        section_specimen: None,
    };
    let expected_result = DiscoveryResult {
        candidates: vec![expected_candidate],
        proposed_groups: Vec::<CandidateGroup>::new(),
        analyzed_lines: 1,
    };
    let expected_document_debug = format!("{document:#?}");
    let expected_settings_debug = format!("{settings:#?}");
    let documents_arg = json!({"kind":"data", "value":[{
        "index":0,
        "document":{"kind":"data", "value":{
            "format":"rust_debug",
            "value":expected_document_debug,
            "native_path":trace_path(&document.path),
        }}
    }]});
    let extracted_arg = json!({"kind":"data", "value":[{
        "index":0,
        "document_id":"doc-1",
        "text":{"kind":"data", "value":{"format":"text_redacted_known_secret_assignments", "value":"Hb: 13.7 g/dL\n"}},
        "warnings":[],
        "page_spans":[{"page_number":2, "character_start":0, "character_end":13}],
    }]});
    let hrefs_arg = json!({"kind":"data", "value":{"doc-1":"../documents/doc-1.txt"}});
    let settings_arg = trace_debug(&expected_settings_debug);
    let accepted_arg = json!({"kind":"data", "value":["hb"]});

    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");
    let actual = session
        .scoped(|| {
            discover_configured_candidates(
                &documents,
                &extracted,
                &accepted_labels,
                Some(&hrefs),
                &settings,
            )
        })
        .expect("scoperta sintetica");
    assert_eq!(actual, expected_result);
    session.finish(Component::Core).expect("chiusura sessione");

    let records = trace_records(&log_path);
    let configured_args = json!([
        argument("documents", documents_arg.clone()),
        argument("extracted_text", extracted_arg.clone()),
        argument("accepted_labels", json!({"kind":"data", "value":["Hb"]})),
        argument("document_hrefs", hrefs_arg.clone()),
        argument("settings", settings_arg.clone()),
    ]);
    let inner_args = json!([
        argument("documents", documents_arg),
        argument("extracted_text", extracted_arg),
        argument("document_hrefs", hrefs_arg),
        argument("settings", settings_arg),
        argument("accepted_labels", accepted_arg),
    ]);
    let expected_output = json!({
        "kind":"data",
        "value":{"format":"rust_debug", "value":format!("{:#?}", Ok::<_, String>(expected_result.clone()))}
    });
    let configured_id = span(
        &records,
        "parameter_slices::discover_configured_candidates",
        configured_args,
        expected_output.clone(),
    );
    let inner_id = span(
        &records,
        "parameter_slices::discover_candidates",
        inner_args,
        expected_output,
    );
    assert_ne!(configured_id, inner_id);
}
