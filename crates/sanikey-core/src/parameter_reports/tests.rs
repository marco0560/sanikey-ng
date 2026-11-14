//! Fixture sintetiche per i report dei parametri.

use super::{
    ParameterDiagnosticExtra, write_parameter_extraction_reports, write_parameter_rejections,
    write_parameter_reports,
};
use crate::logging::{Component, Level, Session};
use crate::metadata::{DocumentRecord, ObservationPoint, ObservationSeries};
use crate::parameter_rules::{ParameterBuildResult, RuleDecision};
use crate::parameter_slices::{
    CandidateGroup, DiscoverySettings, ParameterCandidate, discover_candidates,
};
use crate::pdf::ExtractedText;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

/// Gestisce la directory temporanea dei report di test.
impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sanikey-parameter-reports-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("directory");
        Self(path)
    }
}

/// Rimuove i report creati dalle fixture.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn document(id: &str, date: &str) -> DocumentRecord {
    DocumentRecord {
        document_id: id.into(),
        patient_id: "patient-a".into(),
        path: PathBuf::from(format!("/{id}.txt")),
        title: "Referto sintetico".into(),
        category: "Laboratorio".into(),
        kind: "text".into(),
        sha256: "a".repeat(64),
        date: Some(date.into()),
        series: None,
        tags: Vec::new(),
        origin: "source".into(),
        container_id: None,
        internal_path: None,
    }
}

fn traced<R>(operation: impl FnOnce() -> R) -> (R, String, Vec<Value>) {
    let directory = TestDirectory::new();
    let log_directory = directory.0.join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");
    let result = session.scoped(operation);
    session.finish(Component::Core).expect("chiusura Trace");
    let contents = fs::read_to_string(log_path).expect("log Trace sintetico");
    let records = contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
        .collect();
    (result, contents, records)
}

fn function_span<'a>(records: &'a [Value], function: &str) -> (&'a Value, &'a Value) {
    let enters: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(enters.len(), 1, "ingresso univoco di {function}");
    let enter = enters[0];
    let exits: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_exit"
                && record["function"] == function
                && record["call_id"] == enter["call_id"]
        })
        .collect();
    assert_eq!(exits.len(), 1, "uscita correlata di {function}");
    let exit = exits[0];
    assert_eq!(exit["outcome"], "returned");
    assert!(exit["duration_ms"].as_u64().is_some());
    (enter, exit)
}

fn expected_debug(value: &str) -> Value {
    json!({
        "kind":"data",
        "value":{"format":"rust_debug", "value":value}
    })
}

fn expected_debug_redacted(value: &impl std::fmt::Debug) -> Value {
    expected_debug(&format!("{value:#?}").replace("synthetic-report-secret", "[secret_omitted]"))
}

fn candidate_fixture() -> ParameterCandidate {
    ParameterCandidate {
        stable_id: "candidate-stable-01".into(),
        document_id: "document-02".into(),
        document_date: None,
        document_href: Some("../documents/source-03.pdf".into()),
        document_title: "Synthetic title 04".into(),
        document_name: "source-05.pdf".into(),
        document_category: "Synthetic category 06".into(),
        source_text_digest: "a".repeat(64),
        line_number: 7,
        character_start: 8,
        character_end: 9,
        original_line: "Hb: 13.7 g/dL; api_key=synthetic-report-secret".into(),
        normalized_label: "hb".into(),
        raw_value: "13.7".into(),
        parsed_value: Some(13.7),
        number_format: None,
        qualifier: Some("<".into()),
        raw_unit: Some("g/dL".into()),
        prefix_context: "prefix 10".into(),
        suffix_context: "suffix 11".into(),
        reason_code: None,
        page_number: Some(12),
        context: "context 13; api_key=synthetic-report-secret".into(),
        page_label: "page 14".into(),
        section_specimen: None,
    }
}

fn expected_candidate() -> Value {
    json!({
        "stable_id":"candidate-stable-01",
        "document_id":"document-02",
        "document_date":null,
        "document_href":"../documents/source-03.pdf",
        "document_title":"Synthetic title 04",
        "document_name":"source-05.pdf",
        "document_category":"Synthetic category 06",
        "source_text_digest":"a".repeat(64),
        "line_number":7,
        "character_start":8,
        "character_end":9,
        "original_line":"Hb: 13.7 g/dL; api_key=synthetic-report-secret",
        "normalized_label":"hb",
        "raw_value":"13.7",
        "parsed_value":13.7,
        "number_format":null,
        "qualifier":"<",
        "raw_unit":"g/dL",
        "prefix_context":"prefix 10",
        "suffix_context":"suffix 11",
        "reason_code":null,
        "page_number":12,
        "context":"context 13; api_key=synthetic-report-secret",
        "page_label":"page 14",
        "section_specimen":null
    })
}

#[test]
fn writes_candidate_report_and_disabled_toml_scaffold() {
    let directory = TestDirectory::new();
    let discovery = discover_candidates(
        &[
            document("doc-a", "2026-01-02"),
            document("doc-b", "2026-02-03"),
        ],
        &[
            ExtractedText {
                document_id: "doc-a".into(),
                text: "Hb: 13.7 g/dL\n".into(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            },
            ExtractedText {
                document_id: "doc-b".into(),
                text: "Hb: <14.1 g/dL\n".into(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            },
        ],
        None,
        &DiscoverySettings::default(),
        None,
    )
    .expect("discovery");

    let paths = write_parameter_reports(&directory.0, &discovery, None).expect("reports");

    assert_eq!(
        paths.candidates,
        directory.0.join("reports/parameter-candidates.json")
    );
    assert_eq!(
        paths.proposed_rules,
        directory.0.join("reports/parameter-rules.proposed.toml")
    );
    assert_eq!(paths.slices_json, None);
    assert_eq!(paths.slices_javascript, None);
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&paths.candidates).expect("candidate report"))
            .expect("JSON report");
    assert_eq!(report["schema_version"], "1.0");
    assert_eq!(report["analyzed_lines"], 2);
    assert_eq!(
        report["candidates"].as_array().expect("candidates").len(),
        2
    );
    assert_eq!(report["candidates"][0]["raw_value"], "13.7");

    let proposal = fs::read_to_string(directory.0.join("reports/parameter-rules.proposed.toml"))
        .expect("proposal");
    let parsed: toml::Value = toml::from_str(&proposal).expect("valid proposal TOML");
    assert!(parsed["generated_at"].as_str().is_some());
    let rule = &parsed["parameters"]["hb"];
    assert_eq!(rule["enabled"].as_bool(), Some(false));
    assert_eq!(rule["value_type"].as_str(), Some("qualified-scalar"));
    assert_eq!(rule["number_formats"][0].as_str(), Some("decimal-point"));
    assert_eq!(rule["units"][0].as_str(), Some("g/dL"));
}

#[test]
fn writes_machine_and_javascript_slice_exports_with_stable_bytes() {
    let directory = TestDirectory::new();
    let result = ParameterBuildResult {
        series: vec![ObservationSeries {
            id: "emoglobina".into(),
            name: "Emoglobina".into(),
            value_type: "scalar".into(),
            unit: Some("g/dL".into()),
            description: None,
            warn_duplicate_same_day: true,
            synonyms: vec!["hb".into()],
            parameter_rule_id: Some("emoglobina".into()),
            parameter_rule_version: Some(1),
            parameter_rule_digest: Some("a".repeat(64)),
            unit_variant: None,
        }],
        points: vec![ObservationPoint {
            id: "point-1".into(),
            series_id: "emoglobina".into(),
            observation_date: "2026-01-02".into(),
            source_type: "document-extraction".into(),
            source_reference: "referto.txt".into(),
            numeric_value: Some(13.7),
            text_value: None,
            systolic: None,
            diastolic: None,
            pulse: None,
            note: None,
            source_kind: "document-extraction".into(),
            document_id: Some("doc-a".into()),
            document_href: Some("../documents/referto.txt".into()),
            document_title: Some("Referto sintetico".into()),
            document_category: Some("Laboratorio".into()),
            source_text_digest: Some("b".repeat(64)),
            original_line: Some("Hb: 13.7 g/dL".into()),
            line_number: Some(1),
            page_number: None,
            character_start: Some(0),
            character_end: Some(13),
            matched_label: Some("hb".into()),
            raw_value: Some("13.7".into()),
            parsed_value: Some(13.7),
            raw_unit: Some("g/dL".into()),
            normalized_unit: Some("g/dL".into()),
            qualifier: None,
            rule_id: Some("emoglobina".into()),
            rule_version: Some(1),
            rule_digest: Some("a".repeat(64)),
            reason_code: Some("ACCEPTED_CONFIGURED_SYNONYM".into()),
        }],
        decisions: vec![RuleDecision {
            candidate_id: "candidate-1".into(),
            rule_id: "emoglobina".into(),
            accepted: true,
            reason_code: "ACCEPTED_CONFIGURED_SYNONYM".into(),
        }],
    };

    let (json_path, script_path) =
        write_parameter_extraction_reports(&directory.0, &result).expect("exports");
    let first = fs::read(&json_path).expect("JSON bytes");
    write_parameter_extraction_reports(&directory.0, &result).expect("repeat exports");
    assert_eq!(first, fs::read(&json_path).expect("repeat JSON bytes"));
    let payload: serde_json::Value = serde_json::from_slice(&first).expect("slice JSON");
    assert_eq!(payload["schema_version"], "1.0");
    assert_eq!(payload["series"][0]["parameter_rule_id"], "emoglobina");
    assert_eq!(
        payload["points"][0]["document_href"],
        "../documents/referto.txt"
    );
    assert_eq!(payload["decisions"][0]["accepted"], true);
    let script = fs::read_to_string(script_path).expect("JavaScript export");
    assert!(script.starts_with("window.SANIKEY_PARAMETER_SLICES = {"));
    assert!(script.ends_with(";\n"));
}

#[test]
fn writes_accepted_rejected_json_and_aligned_text_with_extra_diagnostics() {
    let directory = TestDirectory::new();
    let discovery = discover_candidates(
        &[
            document("doc-a", "2026-01-02"),
            document("doc-b", "2026-02-03"),
        ],
        &[
            ExtractedText {
                document_id: "doc-a".into(),
                text: "Hb: 13.7 g/dL\n".into(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            },
            ExtractedText {
                document_id: "doc-b".into(),
                text: "Hb: <14.1 g/dL\n".into(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            },
        ],
        None,
        &DiscoverySettings::default(),
        None,
    )
    .expect("discovery");
    let result = ParameterBuildResult {
        series: Vec::new(),
        points: Vec::new(),
        decisions: vec![
            RuleDecision {
                candidate_id: discovery.candidates[0].stable_id.clone(),
                rule_id: "emoglobina".into(),
                accepted: true,
                reason_code: "ACCEPTED_CONFIGURED_SYNONYM".into(),
            },
            RuleDecision {
                candidate_id: discovery.candidates[1].stable_id.clone(),
                rule_id: "emoglobina".into(),
                accepted: false,
                reason_code: "REJECTED_SPECIMEN_AMBIGUOUS".into(),
            },
        ],
    };
    let extra = [ParameterDiagnosticExtra {
        candidate_id: String::new(),
        parameter: "peso".into(),
        document: "altro-referto.txt".into(),
        line: 3,
        value: "Peso: 5 kg".into(),
        reason_code: "REJECTED_OUT_OF_RANGE".into(),
    }];

    let rejected_paths = write_parameter_rejections(&directory.0, &discovery, &result, &extra)
        .expect("decision reports");

    assert_eq!(
        rejected_paths.0,
        directory.0.join("reports/parameter-rejected.json")
    );
    assert_eq!(
        rejected_paths.1,
        directory.0.join("reports/parameter-rejected.txt")
    );
    let accepted: serde_json::Value = serde_json::from_slice(
        &fs::read(directory.0.join("reports/parameter-accepted.json")).expect("accepted JSON"),
    )
    .expect("accepted report");
    let rejected: serde_json::Value =
        serde_json::from_slice(&fs::read(&rejected_paths.0).expect("rejected JSON"))
            .expect("rejected report");
    assert_eq!(
        accepted["accepted"]
            .as_array()
            .expect("accepted rows")
            .len(),
        1
    );
    assert_eq!(
        rejected["rejected"]
            .as_array()
            .expect("rejected rows")
            .len(),
        2
    );
    assert_eq!(rejected["rejected"][1]["parameter"], "peso");
    let accepted_text = fs::read_to_string(directory.0.join("reports/parameter-accepted.txt"))
        .expect("accepted text");
    let rejected_text = fs::read_to_string(&rejected_paths.1).expect("rejected text");
    assert!(accepted_text
        .lines()
        .next()
        .is_some_and(|header| header.starts_with("Parametro") && header.contains("| Documento")));
    assert!(accepted_text.contains("ACCEPTED_CONFIGURED_SYNONYM"));
    assert!(rejected_text.contains("REJECTED_SPECIMEN_AMBIGUOUS"));
    assert!(rejected_text.contains("REJECTED_OUT_OF_RANGE"));
}

#[test]
fn report_trace_is_private_redacts_secret_assignments_and_preserves_written_artifacts() {
    for level in [Level::Events, Level::Trace] {
        let directory = TestDirectory::new();
        let mut discovery = discover_candidates(
            &[
                document("doc-a", "2026-01-02"),
                document("doc-b", "2026-02-03"),
            ],
            &[
                ExtractedText {
                    document_id: "doc-a".into(),
                    text: "Hb: 13.7 g/dL\n".into(),
                    warnings: Vec::new(),
                    page_spans: Vec::new(),
                },
                ExtractedText {
                    document_id: "doc-b".into(),
                    text: "Hb: 14.1 g/dL\n".into(),
                    warnings: Vec::new(),
                    page_spans: Vec::new(),
                },
            ],
            None,
            &DiscoverySettings::default(),
            None,
        )
        .expect("synthetic discovery");
        let secret_line = "Hb: 13.7 g/dL; password = synthetic-report-secret";
        discovery.candidates[0].original_line = secret_line.into();
        discovery.candidates[0].context = secret_line.into();
        for group in &mut discovery.proposed_groups {
            for candidate in &mut group.candidates {
                if candidate.document_id == "doc-a" {
                    candidate.original_line = secret_line.into();
                    candidate.context = secret_line.into();
                }
            }
        }

        let log_directory = directory.0.join("logs");
        let (session, log_path) =
            Session::create_in_directory_with_level(&log_directory, Component::Core, level)
                .expect("synthetic logging session");
        let reports = directory.0.join("build");
        session.scoped(|| {
            let paths = write_parameter_reports(&reports, &discovery, None)
                .expect("synthetic parameter reports");
            let written = fs::read_to_string(paths.candidates).expect("candidate JSON");
            assert!(written.contains("synthetic-report-secret"));
        });
        session.finish(Component::Core).expect("finish log session");
        drop(session);

        let contents = fs::read_to_string(log_path).expect("read synthetic log");
        if level == Level::Trace {
            assert!(contents.contains("parameter_reports::write_parameter_reports"));
            assert!(contents.contains("parameter_reports::write_json"));
            assert!(contents.contains("Hb: 13.7 g/dL"));
            assert!(contents.contains("secret_omitted"));
            assert!(!contents.contains("synthetic-report-secret"));
        } else {
            assert!(!contents.contains("parameter_reports::"));
            assert!(!contents.contains("Hb: 13.7 g/dL"));
        }
    }
}

#[cfg(unix)]
#[test]
fn report_path_trace_keeps_non_unicode_path_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let path = PathBuf::from(OsString::from_vec(b"/synthetic/report-\xff.json".to_vec()));
    let result = Ok(super::ParameterReportPaths {
        candidates: path.clone(),
        proposed_rules: path.clone(),
        slices_json: Some(path.clone()),
        slices_javascript: Some(path),
    });
    let encoded = super::trace_parameter_report_paths(&result).to_json();
    assert_eq!(
        encoded["value"]["paths"]["candidates"]["value"]["encoding"],
        "unix_bytes"
    );
    assert!(
        encoded["value"]["paths"]["candidates"]["value"]["value"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(0xff))
    );
}

#[test]
fn synthetic_trace_records_complete_parameter_report_projections() {
    let candidate = candidate_fixture();
    let expected_candidate = expected_candidate();
    let (candidate_value, candidate_log, candidate_records) =
        traced(|| super::candidate_value(&candidate));
    assert_eq!(candidate_value, expected_candidate);
    assert_eq!(candidate_value["original_line"], candidate.original_line);
    assert_eq!(candidate_value["context"], candidate.context);
    assert!(candidate_log.contains("secret_omitted"));
    assert!(!candidate_log.contains("synthetic-report-secret"));
    let (enter, exit) = function_span(&candidate_records, "parameter_reports::candidate_value");
    assert_eq!(
        enter["arguments"],
        json!([{"name":"candidate", "content":expected_debug_redacted(&candidate)}])
    );
    assert_eq!(exit["result"], expected_debug_redacted(&expected_candidate));

    let group = CandidateGroup {
        normalized_label: "hb".into(),
        candidates: vec![candidate.clone()],
    };
    let expected_group = json!({"normalized_label":"hb", "candidates":[expected_candidate]});
    let (group_value, group_log, group_records) = traced(|| super::group_value(&group));
    assert_eq!(group_value, expected_group);
    assert!(!group_log.contains("synthetic-report-secret"));
    let (enter, exit) = function_span(&group_records, "parameter_reports::group_value");
    assert_eq!(
        enter["arguments"],
        json!([{"name":"group", "content":expected_debug_redacted(&group)}])
    );
    assert_eq!(exit["result"], expected_debug_redacted(&expected_group));

    let series = ObservationSeries {
        id: "series-01".into(),
        name: "Series 02".into(),
        value_type: "numeric-03".into(),
        unit: Some("unit-04".into()),
        description: None,
        warn_duplicate_same_day: true,
        synonyms: vec!["alias-06".into(), "alias-07".into()],
        parameter_rule_id: Some("rule-08".into()),
        parameter_rule_version: Some(9),
        parameter_rule_digest: Some("b".repeat(64)),
        unit_variant: None,
    };
    let expected_series = json!({
        "id":"series-01", "name":"Series 02", "value_type":"numeric-03",
        "unit":"unit-04", "description":null,
        "warn_duplicate_same_day":true, "synonyms":["alias-06", "alias-07"],
        "parameter_rule_id":"rule-08", "parameter_rule_version":9,
        "parameter_rule_digest":"b".repeat(64), "unit_variant":null
    });
    let (series_value, _, series_records) = traced(|| super::series_value(&series));
    assert_eq!(series_value, expected_series);
    let (enter, exit) = function_span(&series_records, "parameter_reports::series_value");
    assert_eq!(
        enter["arguments"],
        json!([{"name":"series", "content":expected_debug(&format!("{series:#?}"))}])
    );
    assert_eq!(
        exit["result"],
        expected_debug(&format!("{expected_series:#?}"))
    );

    let point = ObservationPoint {
        id: "point-01".into(),
        series_id: "series-02".into(),
        observation_date: "2026-04-05".into(),
        source_type: "source-type-04".into(),
        source_reference: "source-reference-05".into(),
        numeric_value: Some(6.5),
        text_value: None,
        systolic: Some(8.5),
        diastolic: Some(9.5),
        pulse: Some(10.5),
        note: None,
        source_kind: "source-kind-12".into(),
        document_id: Some("document-13".into()),
        document_href: Some("href-14".into()),
        document_title: Some("title-15".into()),
        document_category: Some("category-16".into()),
        source_text_digest: Some("c".repeat(64)),
        original_line: Some("line-18".into()),
        line_number: Some(19),
        page_number: None,
        character_start: Some(21),
        character_end: Some(22),
        matched_label: Some("label-23".into()),
        raw_value: Some("raw-24".into()),
        parsed_value: Some(25.5),
        raw_unit: Some("raw-unit-26".into()),
        normalized_unit: Some("normalized-unit-27".into()),
        qualifier: None,
        rule_id: Some("rule-29".into()),
        rule_version: Some(30),
        rule_digest: Some("d".repeat(64)),
        reason_code: Some("reason-32".into()),
    };
    let expected_point = json!({
        "id":"point-01", "series_id":"series-02", "observation_date":"2026-04-05",
        "source_type":"source-type-04", "source_reference":"source-reference-05",
        "numeric_value":6.5, "text_value":null, "systolic":8.5,
        "diastolic":9.5, "pulse":10.5, "note":null, "source_kind":"source-kind-12",
        "document_id":"document-13", "document_href":"href-14", "document_title":"title-15",
        "document_category":"category-16", "source_text_digest":"c".repeat(64),
        "original_line":"line-18", "line_number":19, "page_number":null,
        "character_start":21, "character_end":22, "matched_label":"label-23",
        "raw_value":"raw-24", "parsed_value":25.5, "raw_unit":"raw-unit-26",
        "normalized_unit":"normalized-unit-27", "qualifier":null,
        "rule_id":"rule-29", "rule_version":30, "rule_digest":"d".repeat(64),
        "reason_code":"reason-32"
    });
    let (point_value, _, point_records) = traced(|| super::point_value(&point));
    assert_eq!(point_value, expected_point);
    let (enter, exit) = function_span(&point_records, "parameter_reports::point_value");
    assert_eq!(
        enter["arguments"],
        json!([{"name":"point", "content":expected_debug(&format!("{point:#?}"))}])
    );
    assert_eq!(
        exit["result"],
        expected_debug(&format!("{expected_point:#?}"))
    );

    for (cell_input, expected_cell) in [
        (Value::Null, "None"),
        (json!("cell text"), "cell text"),
        (json!(17), "17"),
        (json!([1, "x"]), "[1,\"x\"]"),
        (json!({"field":"value"}), "{\"field\":\"value\"}"),
    ] {
        let (cell, _, cell_records) = traced(|| super::json_cell(&cell_input));
        assert_eq!(cell, expected_cell);
        let (enter, exit) = function_span(&cell_records, "parameter_reports::json_cell");
        assert_eq!(
            enter["arguments"],
            json!([{"name":"value", "content":{"kind":"data", "value":cell_input}}])
        );
        assert_eq!(
            exit["result"],
            expected_debug(&format!("{expected_cell:?}"))
        );
    }

    let row = [
        "A".into(),
        "BB".into(),
        "C".into(),
        "DD".into(),
        "E".into(),
        "FF".into(),
        "G".into(),
    ];
    let widths = [2, 2, 1, 2, 1, 2, 1];
    let (rendered, _, row_records) = traced(|| super::render_table_row(&row, &widths));
    let expected_row = "A  | BB | C | DD | E | FF | G";
    assert_eq!(rendered, expected_row);
    let (enter, exit) = function_span(&row_records, "parameter_reports::render_table_row");
    assert_eq!(
        enter["arguments"],
        json!([
            {"name":"row", "content":expected_debug(&format!("{row:#?}"))},
            {"name":"widths", "content":expected_debug(&format!("{widths:#?}"))}
        ])
    );
    assert_eq!(
        exit["result"],
        expected_debug("\"A  | BB | C | DD | E | FF | G\"")
    );
}
