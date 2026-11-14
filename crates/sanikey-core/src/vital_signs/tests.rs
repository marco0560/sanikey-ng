//! Fixture sintetiche per estrazione e merge dei segni vitali.

use super::{extract_vital_signs, merge_vital_signs};
use crate::logging::{Component, Level, Session};
use crate::metadata::{CuratedMetadata, DocumentRecord, ObservationPoint, ObservationSeries};
use crate::pdf::{ExtractedText, TextPageSpan};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_LOG_ID: AtomicU64 = AtomicU64::new(0);

fn document(id: &str, date: Option<&str>) -> DocumentRecord {
    DocumentRecord {
        document_id: id.into(),
        patient_id: "synthetic-person".into(),
        path: PathBuf::from(format!("/{id}.txt")),
        title: format!("Referto {id}"),
        category: "Visita".into(),
        kind: "text".into(),
        sha256: "a".repeat(64),
        date: date.map(str::to_owned),
        series: None,
        tags: Vec::new(),
        origin: "source".into(),
        container_id: None,
        internal_path: None,
    }
}

fn extraction(id: &str, text: &str) -> ExtractedText {
    ExtractedText {
        document_id: id.into(),
        text: text.into(),
        warnings: Vec::new(),
        page_spans: Vec::new(),
    }
}

fn expected_debug(value: &impl std::fmt::Debug, secret: &str) -> Value {
    let rendered = format!("{value:#?}").replace(secret, "[secret_omitted]");
    json!({
        "kind": "data",
        "value": {
            "format": "text_redacted_known_secret_assignments",
            "value": rendered,
        },
    })
}

fn expected_sensitive_text(value: &str, secret: &str) -> Value {
    json!({
        "kind": "data",
        "value": {
            "format": "text_redacted_known_secret_assignments",
            "value": value.replace(secret, "[secret_omitted]"),
        },
    })
}

fn expected_data(value: Value) -> Value {
    json!({"kind": "data", "value": value})
}

fn assert_vital_spans(
    records: &[Value],
    function: &str,
    mut expected: Vec<(Value, Value)>,
) -> Vec<u64> {
    let entries = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        expected.len(),
        "numero di chiamate a {function}"
    );

    let mut unmatched = entries;
    let mut call_ids = Vec::new();
    for (arguments, result) in expected.drain(..) {
        let index = unmatched
            .iter()
            .position(|entry| entry["arguments"] == arguments)
            .unwrap_or_else(|| {
                panic!(
                    "argomenti attesi assenti per {function}: {arguments}; presenti: {:?}",
                    unmatched
                        .iter()
                        .map(|entry| &entry["arguments"])
                        .collect::<Vec<_>>()
                )
            });
        let entry = unmatched.remove(index);
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        let exits = records
            .iter()
            .filter(|record| {
                record["type"] == "function_exit"
                    && record["function"] == function
                    && record["call_id"] == call_id
            })
            .collect::<Vec<_>>();
        assert_eq!(exits.len(), 1, "uscita correlata per {function}#{call_id}");
        assert_eq!(exits[0]["outcome"], "returned");
        assert_eq!(
            exits[0]["result"], result,
            "risultato di {function}#{call_id}"
        );
        assert!(
            exits[0]["duration_ms"].as_u64().is_some(),
            "durata numerica per {function}#{call_id}"
        );
        call_ids.push(call_id);
    }
    assert!(unmatched.is_empty());
    call_ids
}

#[test]
fn extracts_only_explicit_weight_and_pressure_forms_with_source_provenance() {
    let extracted = ExtractedText {
        page_spans: vec![TextPageSpan {
            page_number: 2,
            character_start: 0,
            character_end: 80,
        }],
        ..extraction(
            "doc-a",
            "Peso corporeo: 70,5 kg\nPA: 120/80 mm Hg\nPeso 70\nPeso: 19 kg",
        )
    };
    let hrefs = HashMap::from([("doc-a".into(), "../documents/referto.txt".into())]);

    let result = extract_vital_signs(
        &[document("doc-a", Some("2026-01-02"))],
        &[extracted],
        Some(&hrefs),
    );

    assert_eq!(
        result
            .series
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["peso", "pressione"]
    );
    assert_eq!(result.points.len(), 2);
    assert_eq!(result.points[0].numeric_value, Some(70.5));
    assert_eq!(result.points[1].systolic, Some(120.0));
    assert_eq!(result.points[1].diastolic, Some(80.0));
    assert_eq!(result.points[0].observation_date, "2026-01-02");
    assert_eq!(
        result.points[0].document_href.as_deref(),
        Some("../documents/referto.txt")
    );
    assert_eq!(result.points[0].page_number, Some(2));
    assert_eq!(
        result.points[0].reason_code.as_deref(),
        Some("ACCEPTED_STRICT_VITAL_SIGN")
    );
    assert_eq!(result.rejections.len(), 1);
    assert_eq!(result.rejections[0].parameter, "peso");
    assert_eq!(result.rejections[0].line, 4);
    assert_eq!(result.rejections[0].reason_code, "REJECTED_OUT_OF_RANGE");
}

#[test]
fn ignores_undated_and_unmatched_documents_and_rejects_invalid_pressure() {
    let result = extract_vital_signs(
        &[
            document("undated", None),
            document("dated", Some("2026-01-02")),
        ],
        &[
            extraction("missing", "Peso: 70 kg"),
            extraction("undated", "Peso: 70 kg"),
            extraction("dated", "PA: 80/90 mmHg\nPA: 300/20 mmHg"),
        ],
        None,
    );

    assert!(result.series.is_empty());
    assert!(result.points.is_empty());
    assert_eq!(result.rejections.len(), 2);
    assert!(
        result
            .rejections
            .iter()
            .all(|item| item.parameter == "pressione")
    );
}

#[test]
fn extraction_is_deterministic_under_input_permutation() {
    let documents = [
        document("doc-b", Some("2026-01-03")),
        document("doc-a", Some("2026-01-02")),
    ];
    let texts = [
        extraction("doc-b", "Pressione arteriosa 125/85 mmHg\nPeso: 71 kg"),
        extraction("doc-a", "Peso: 70 kg"),
    ];

    let first = extract_vital_signs(&documents, &texts, None);
    let second = extract_vital_signs(
        &[documents[1].clone(), documents[0].clone()],
        &[texts[1].clone(), texts[0].clone()],
        None,
    );

    assert_eq!(first, second);
}

#[test]
fn merge_preserves_same_day_curated_points_and_deduplicates_series() {
    let result = extract_vital_signs(
        &[
            document("doc-a", Some("2026-01-02")),
            document("doc-b", Some("2026-01-03")),
        ],
        &[
            extraction("doc-a", "Peso: 70 kg\nPA: 120/80 mmHg"),
            extraction("doc-b", "Peso: 71 kg"),
        ],
        None,
    );
    let curated = ObservationPoint {
        id: "curated-weight".into(),
        series_id: "peso".into(),
        observation_date: "2026-01-02".into(),
        source_type: "csv".into(),
        source_reference: "peso.csv:2".into(),
        numeric_value: Some(72.0),
        text_value: None,
        systolic: None,
        diastolic: None,
        pulse: None,
        note: None,
        source_kind: "curated-observation".into(),
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
    let metadata = CuratedMetadata {
        observation_series: vec![ObservationSeries {
            id: "peso".into(),
            name: "Peso curato".into(),
            value_type: "numeric".into(),
            unit: Some("kg".into()),
            description: None,
            warn_duplicate_same_day: false,
            synonyms: Vec::new(),
            parameter_rule_id: None,
            parameter_rule_version: None,
            parameter_rule_digest: None,
            unit_variant: None,
        }],
        observation_points: vec![curated.clone()],
        ..CuratedMetadata::default()
    };

    let merged = merge_vital_signs(&metadata, &result);

    assert_eq!(merged.observation_points.len(), 3);
    assert_eq!(merged.observation_points[0], curated);
    assert_eq!(
        merged
            .observation_points
            .iter()
            .filter(|point| point.series_id == "peso")
            .count(),
        2
    );
    assert_eq!(merged.observation_series.len(), 2);
    assert_eq!(merged.observation_series[0].name, "Peso curato");
    assert_eq!(merged.observation_series[1].id, "pressione");
}

#[test]
fn trace_captures_private_text_and_redacts_identified_secrets_across_extraction_and_merge() {
    let secret = "synthetic-secret-vital-7462";
    let private_line = "Nota sanitaria privata sintetica 2841";
    let accepted_line = format!("Peso: 70 kg; password = \"{secret}\"");
    let rejected_line = format!("Peso: 999 kg; token = \"{secret}\"");
    let directory = std::env::temp_dir().join(format!(
        "sanikey-vital-trace-{}-{}",
        std::process::id(),
        NEXT_LOG_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let (session, log_path) =
        Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");
    let mut source_document = document("trace-doc", Some("2026-01-02"));
    source_document.title = format!("Referto password={secret}");
    let source_text = format!("{private_line}\n{accepted_line}\n{rejected_line}");

    let (result, merged) = session.scoped(|| {
        let result = extract_vital_signs(
            &[source_document],
            &[extraction("trace-doc", &source_text)],
            None,
        );
        let merged = merge_vital_signs(&CuratedMetadata::default(), &result);
        (result, merged)
    });
    assert_eq!(result.points.len(), 1);
    assert_eq!(
        result.points[0].original_line.as_deref(),
        Some(accepted_line.as_str())
    );
    assert_eq!(result.rejections.len(), 1);
    assert_eq!(result.rejections[0].value, rejected_line);
    assert_eq!(merged.observation_points, result.points);
    drop(session);

    let log = fs::read_to_string(&log_path).expect("log Trace sintetico");
    assert!(log.contains(private_line));
    assert!(log.contains("secret_omitted"));
    assert!(
        !log.contains(secret),
        "secret escaped through trace record(s): {}",
        log.lines()
            .filter(|line| line.contains(secret))
            .collect::<Vec<_>>()
            .join("\n")
    );
    fs::remove_dir_all(directory).expect("rimozione log sintetico");
}

#[test]
fn events_level_does_not_record_extracted_private_text() {
    let private_text = "Nota sanitaria privata sintetica 9276";
    let directory = std::env::temp_dir().join(format!(
        "sanikey-vital-events-{}-{}",
        std::process::id(),
        NEXT_LOG_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let (session, log_path) =
        Session::create_in_directory_with_level(&directory, Component::Core, Level::Events)
            .expect("sessione Events sintetica");

    let result = session.scoped(|| {
        extract_vital_signs(
            &[document("events-doc", Some("2026-01-02"))],
            &[extraction("events-doc", private_text)],
            None,
        )
    });
    assert!(result.points.is_empty());
    drop(session);

    let log = fs::read_to_string(&log_path).expect("log Events sintetico");
    assert!(!log.contains(private_text));
    fs::remove_dir_all(directory).expect("rimozione log sintetico");
}

#[test]
fn trace_spans_match_vital_sign_arguments_results_ids_durations_and_redactions() {
    let secret = "synthetic-vital-trace-secret-4817";
    let source_text =
        format!("Peso: 70 kg\nPressione: 120/80 mmHg\nPeso: 999 kg; token = {secret}");
    let page_spans = vec![TextPageSpan {
        page_number: 2,
        character_start: 0,
        character_end: source_text.chars().count(),
    }];
    let extracted = ExtractedText {
        page_spans: page_spans.clone(),
        ..extraction("trace-vital-doc", &source_text)
    };
    let documents = vec![document("trace-vital-doc", Some("2026-01-02"))];
    let extractions = vec![extracted];
    let hrefs = HashMap::from([(
        "trace-vital-doc".to_owned(),
        "../documents/referto-sintetico.txt".to_owned(),
    )]);
    let directory = std::env::temp_dir().join(format!(
        "sanikey-vital-span-trace-{}-{}",
        std::process::id(),
        NEXT_LOG_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let (session, log_path) =
        Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");

    let (result, merged, direct_point, direct_rejection, series, page_hits, page_miss) = session
        .scoped(|| {
            let result = extract_vital_signs(&documents, &extractions, Some(&hrefs));
            let merged = merge_vital_signs(&CuratedMetadata::default(), &result);
            let weight_point = result
                .points
                .iter()
                .find(|point| point.series_id == "peso")
                .expect("punto peso sintetico");
            let accepted_line = source_text.lines().next().expect("riga peso sintetica");
            let document = &documents[0];
            let extracted = &extractions[0];
            let digest = weight_point
                .source_text_digest
                .as_deref()
                .expect("digest testo sintetico");
            let weight_captures = super::weight_regex()
                .captures(accepted_line)
                .expect("cattura peso sintetica");
            let source = super::PointSource {
                document,
                date: "2026-01-02",
                extracted,
                hrefs: Some(&hrefs),
                text_digest: digest,
                line: accepted_line,
                line_number: 1,
                character_line_start: 0,
            };
            let direct_point = super::build_point(&source, &weight_captures, "peso", 70.0, None);
            let direct_rejection = super::rejection(
                document,
                3,
                source_text
                    .lines()
                    .nth(2)
                    .expect("riga rifiutata sintetica"),
                "peso",
            );
            let series = super::canonical_series();
            let page_hits = super::page_number(&page_spans, 0);
            let page_miss = super::page_number(&page_spans, source_text.chars().count() + 1);
            (
                result,
                merged,
                direct_point,
                direct_rejection,
                series,
                page_hits,
                page_miss,
            )
        });

    let weight_point = result
        .points
        .iter()
        .find(|point| point.series_id == "peso")
        .expect("punto peso sintetico");
    let pressure_point = result
        .points
        .iter()
        .find(|point| point.series_id == "pressione")
        .expect("punto pressione sintetico");
    assert_eq!(result.points.len(), 2);
    assert_eq!(result.series.len(), 2);
    assert_eq!(weight_point.numeric_value, Some(70.0));
    assert_eq!(
        weight_point.document_href.as_deref(),
        Some("../documents/referto-sintetico.txt")
    );
    assert_eq!(weight_point.page_number, Some(2));
    assert_eq!(pressure_point.systolic, Some(120.0));
    assert_eq!(pressure_point.diastolic, Some(80.0));
    assert_eq!(pressure_point.page_number, Some(2));
    assert_eq!(result.rejections.len(), 1);
    assert_eq!(result.rejections[0].parameter, "peso");
    assert_eq!(result.rejections[0].line, 3);
    assert_eq!(direct_point, *weight_point);
    assert_eq!(direct_rejection, result.rejections[0]);
    assert_eq!(merged.observation_points, result.points);
    assert_eq!(series.as_slice(), result.series.as_slice());
    assert_eq!(page_hits, Some(2));
    assert_eq!(page_miss, None);
    drop(session);

    let records = fs::read_to_string(&log_path)
        .expect("log Trace sintetico leggibile")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSON valido"))
        .collect::<Vec<_>>();
    let accepted_line = source_text.lines().next().expect("riga peso sintetica");
    let pressure_line = source_text
        .lines()
        .nth(1)
        .expect("riga pressione sintetica");
    let rejected_line = source_text
        .lines()
        .nth(2)
        .expect("riga rifiutata sintetica");
    let pressure_character_start = source_text
        .find(pressure_line)
        .expect("offset pressione sintetico");
    let weight_captures = super::weight_regex()
        .captures(accepted_line)
        .expect("cattura peso sintetica");
    let pressure_captures = super::pressure_regex()
        .captures(pressure_line)
        .expect("cattura pressione sintetica");
    let weight_match = weight_captures
        .get(0)
        .expect("match peso sintetico")
        .as_str();
    let pressure_match = pressure_captures
        .get(0)
        .expect("match pressione sintetico")
        .as_str();
    let document = &documents[0];
    let weight_point = result
        .points
        .iter()
        .find(|point| point.series_id == "peso")
        .expect("punto peso sintetico");
    let pressure_point = result
        .points
        .iter()
        .find(|point| point.series_id == "pressione")
        .expect("punto pressione sintetico");
    let build_arguments = |line: &str,
                           matched: &str,
                           line_number: u64,
                           character_line_start: u64,
                           series_id: &str,
                           value: f64,
                           diastolic: Option<f64>| {
        json!([
            {"name": "document", "content": expected_debug(document, secret)},
            {"name": "date", "content": expected_sensitive_text("2026-01-02", secret)},
            {"name": "text_digest", "content": expected_data(json!(weight_point.source_text_digest.as_deref().expect("digest peso")))},
            {"name": "line", "content": expected_sensitive_text(line, secret)},
            {"name": "matched_text", "content": expected_sensitive_text(matched, secret)},
            {"name": "line_number", "content": expected_data(json!(line_number))},
            {"name": "character_line_start", "content": expected_data(json!(character_line_start))},
            {"name": "series_id", "content": expected_data(json!(series_id))},
            {"name": "value", "content": expected_data(json!(value))},
            {"name": "diastolic", "content": expected_data(json!(diastolic))},
            {"name": "hrefs", "content": expected_debug(&Some(&hrefs), secret)},
        ])
    };
    let weight_build_arguments =
        build_arguments(accepted_line, weight_match, 1, 0, "peso", 70.0, None);
    let pressure_build_arguments = build_arguments(
        pressure_line,
        pressure_match,
        2,
        pressure_character_start as u64,
        "pressione",
        120.0,
        Some(80.0),
    );
    let weight_point_result = expected_debug(weight_point, secret);
    let pressure_point_result = expected_debug(pressure_point, secret);
    let rejection_arguments = json!([
        {"name": "document", "content": expected_debug(document, secret)},
        {"name": "line_number", "content": expected_data(json!(3_u64))},
        {"name": "line", "content": expected_sensitive_text(rejected_line, secret)},
        {"name": "parameter", "content": expected_data(json!("peso"))},
    ]);
    let rejection_result = expected_debug(&result.rejections[0], secret);

    assert_vital_spans(
        &records,
        "vital_signs::extract_vital_signs",
        vec![(
            json!([
                {"name": "documents", "content": expected_debug(&documents, secret)},
                {"name": "extracted_text", "content": expected_debug(&extractions, secret)},
                {"name": "document_hrefs", "content": expected_debug(&Some(&hrefs), secret)},
            ]),
            expected_debug(&result, secret),
        )],
    );
    assert_vital_spans(
        &records,
        "vital_signs::merge_vital_signs",
        vec![(
            json!([
                {"name": "metadata", "content": expected_debug(&CuratedMetadata::default(), secret)},
                {"name": "result", "content": expected_debug(&result, secret)},
            ]),
            expected_debug(&merged, secret),
        )],
    );
    assert_vital_spans(
        &records,
        "vital_signs::build_point",
        vec![
            (weight_build_arguments.clone(), weight_point_result.clone()),
            (pressure_build_arguments, pressure_point_result),
            (
                weight_build_arguments,
                expected_debug(&direct_point, secret),
            ),
        ],
    );
    assert_vital_spans(
        &records,
        "vital_signs::rejection",
        vec![
            (rejection_arguments.clone(), rejection_result.clone()),
            (
                rejection_arguments,
                expected_debug(&direct_rejection, secret),
            ),
        ],
    );
    assert_vital_spans(
        &records,
        "vital_signs::canonical_series",
        vec![
            (json!([]), expected_debug(&series, secret)),
            (json!([]), expected_debug(&series, secret)),
        ],
    );
    assert_vital_spans(
        &records,
        "vital_signs::page_number",
        vec![
            (
                json!([
                    {"name": "page_spans", "content": expected_debug(&page_spans, secret)},
                    {"name": "character", "content": expected_data(json!(0_u64))},
                ]),
                expected_data(json!(Some(2_usize))),
            ),
            (
                json!([
                    {"name": "page_spans", "content": expected_debug(&page_spans, secret)},
                    {"name": "character", "content": expected_data(json!(pressure_character_start as u64))},
                ]),
                expected_data(json!(Some(2_usize))),
            ),
            (
                json!([
                    {"name": "page_spans", "content": expected_debug(&page_spans, secret)},
                    {"name": "character", "content": expected_data(json!(0_u64))},
                ]),
                expected_data(json!(Some(2_usize))),
            ),
            (
                json!([
                    {"name": "page_spans", "content": expected_debug(&page_spans, secret)},
                    {"name": "character", "content": expected_data(json!(0_u64))},
                ]),
                expected_data(json!(Some(2_usize))),
            ),
            (
                json!([
                    {"name": "page_spans", "content": expected_debug(&page_spans, secret)},
                    {"name": "character", "content": expected_data(json!(source_text.chars().count() as u64 + 1))},
                ]),
                expected_data(json!(None::<usize>)),
            ),
        ],
    );

    let serialized = serde_json::to_string(&records).expect("serializzazione log JSON");
    assert!(serialized.contains("[secret_omitted]"));
    assert!(!serialized.contains(secret));
    assert!(serialized.contains("Peso: 70 kg"));
    assert!(serialized.contains("Pressione: 120/80 mmHg"));
    fs::remove_dir_all(directory).expect("rimozione log sintetico");
}
