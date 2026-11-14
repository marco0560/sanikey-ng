//! Fixture sintetiche per estrazione e merge dei segni vitali.

use super::{extract_vital_signs, merge_vital_signs};
use crate::logging::{Component, Level, Session};
use crate::metadata::{CuratedMetadata, DocumentRecord, ObservationPoint, ObservationSeries};
use crate::pdf::{ExtractedText, TextPageSpan};
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
