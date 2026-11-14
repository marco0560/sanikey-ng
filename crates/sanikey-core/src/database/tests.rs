//! Fixture sintetiche per la persistenza del database.

use super::{build_database, database_path};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::dicom::DicomStudy;
use crate::logging::{Component, Level, Session};
use crate::metadata::{
    ClinicalProblem, CuratedMetadata, DocumentRecord, Medication, Observation, ObservationPoint,
    ObservationSeries, Procedure, TherapyEpisode, TimelineEvent,
};
use crate::pdf::ExtractedText;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

/// Gestisce le directory temporanee dei test database.
impl TestDirectory {
    fn new() -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-database-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("directory sintetica");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Elimina i file di test al termine del test.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(root: &Path) -> PersonConfig {
    PersonConfig {
        id: "patient-a".into(),
        display_name: "Patient A".into(),
        source_documents: root.join("documents"),
        metadata_directory: root.join("metadata"),
        local_build: root.join("generated"),
        usb_uuid: "1A2B-3C4D".into(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn document(root: &Path) -> DocumentRecord {
    DocumentRecord {
        document_id: "doc-a".into(),
        patient_id: "patient-a".into(),
        path: root.join("documents/20260102 Blood Test.txt"),
        title: "Blood Test".into(),
        category: "Laboratorio".into(),
        kind: "text".into(),
        sha256: "synthetic-sha256".into(),
        date: Some("2026-01-02".into()),
        series: None,
        tags: vec!["laboratorio".into()],
        origin: "source".into(),
        container_id: None,
        internal_path: None,
    }
}

fn metadata() -> CuratedMetadata {
    CuratedMetadata {
        problems: vec![ClinicalProblem {
            id: "problem-a".into(),
            title: "Problem A".into(),
            status: "active".into(),
            provenance: vec![],
        }],
        medications: vec![Medication {
            id: "drug-a".into(),
            name: "Drug A".into(),
            active_ingredient: Some("Ingredient A".into()),
            form: Some("compresse".into()),
            strength_per_unit: Some("100 mg".into()),
        }],
        therapies: vec![TherapyEpisode {
            id: "therapy-a".into(),
            medication_id: "drug-a".into(),
            start_date: None,
            end_date: None,
            dosage: None,
            role: None,
            schedule: vec!["risveglio".into(), "cena".into()],
            instructions: Some("dopo il pasto".into()),
        }],
        procedures: vec![Procedure {
            id: "procedure-a".into(),
            title: "Synthetic procedure".into(),
            date: Some("2026-01-02".into()),
            status: "completed".into(),
        }],
        observations: vec![Observation {
            id: "observation-a".into(),
            kind: "weight".into(),
            value: "70 kg".into(),
            date: Some("2026-01-02".into()),
        }],
        observation_series: vec![ObservationSeries {
            id: "hemoglobin".into(),
            name: "Emoglobina".into(),
            value_type: "qualified-scalar".into(),
            unit: Some("g/dL".into()),
            description: None,
            warn_duplicate_same_day: true,
            synonyms: vec!["Hb".into(), "HGB".into()],
            parameter_rule_id: Some("hemoglobin".into()),
            parameter_rule_version: Some(1),
            parameter_rule_digest: Some("rule-digest".into()),
            unit_variant: None,
        }],
        observation_points: vec![ObservationPoint {
            id: "point-a".into(),
            series_id: "hemoglobin".into(),
            observation_date: "2026-01-02".into(),
            source_type: "document-line".into(),
            source_reference: "doc-a:1".into(),
            numeric_value: Some(13.7),
            text_value: None,
            systolic: None,
            diastolic: None,
            pulse: None,
            note: None,
            source_kind: "document-extraction".into(),
            document_id: Some("doc-a".into()),
            document_href: Some("../documents/report.txt".into()),
            document_title: Some("Report".into()),
            document_category: Some("Laboratorio".into()),
            source_text_digest: Some("text-digest".into()),
            original_line: Some("Hb: 137 g/l".into()),
            line_number: Some(1),
            page_number: None,
            character_start: Some(0),
            character_end: Some(11),
            matched_label: Some("Hb".into()),
            raw_value: Some("137".into()),
            parsed_value: Some(137.0),
            raw_unit: Some("g/l".into()),
            normalized_unit: Some("g/dL".into()),
            qualifier: Some("<".into()),
            rule_id: Some("hemoglobin".into()),
            rule_version: Some(1),
            rule_digest: Some("rule-digest".into()),
            reason_code: Some("ACCEPTED_CONFIGURED_SYNONYM".into()),
        }],
        timeline_events: vec![TimelineEvent {
            id: "event-a".into(),
            title: "Synthetic event".into(),
            start_date: Some("2026-01-02".into()),
            end_date: None,
            source: "curated".into(),
            links: vec!["doc-a".into()],
        }],
        ..CuratedMetadata::default()
    }
}

fn study(root: &Path) -> DicomStudy {
    DicomStudy {
        study_id: "study-a".into(),
        patient_id: "patient-a".into(),
        support_path: root.join("documents/study.iso"),
        support_kind: "dicom_study".into(),
        extracted_path: None,
        viewer_paths: vec![],
        html_viewer_path: None,
        warnings: vec![],
        study_instance_uid: Some("1.2.826.0.1.3680043.10.543.1".into()),
        study_date: Some("2026-01-02".into()),
        study_description: Some("Synthetic Study".into()),
        instance_count: 2,
        support_paths: vec![],
    }
}

fn expected_text_trace(value: &str) -> Value {
    json!({
        "kind": "data",
        "value": {
            "format": "text_redacted_known_secret_assignments",
            "value": value,
        }
    })
}

fn expected_path_trace(path: &Path) -> Value {
    json!({
        "kind": "data",
        "value": {"encoding": "utf8", "value": path.to_str().expect("path fixture UTF-8")}
    })
}

fn expected_arguments(fields: Vec<(&str, Value)>) -> Value {
    Value::Array(
        fields
            .into_iter()
            .map(|(name, content)| json!({"name": name, "content": content}))
            .collect(),
    )
}

fn expected_document_trace(path: &Path) -> Value {
    let path_text = path.to_string_lossy();
    json!({
        "kind": "data",
        "value": [{
            "kind": "data",
            "value": {
                "document_id": expected_text_trace("doc-a"),
                "patient_id": expected_text_trace("patient-a"),
                "path": expected_path_trace(path),
                "sqlite_path_text_lossy": expected_text_trace(&path_text),
                "sqlite_path_encoding": "lossy_utf8",
                "title": expected_text_trace("Blood Test"),
                "category": expected_text_trace("Laboratorio"),
                "kind": expected_text_trace("text"),
                "sha256": expected_text_trace("synthetic-sha256"),
                "date": expected_text_trace("2026-01-02"),
                "series": null,
                "tags": [expected_text_trace("laboratorio")],
                "origin": expected_text_trace("source"),
                "container_id": null,
                "internal_path": null,
            }
        }]
    })
}

fn expected_study_trace(support_path: &Path) -> Value {
    json!({
        "kind": "data",
        "value": [{
                "study_id": expected_text_trace("study-a"),
                "patient_id": expected_text_trace("patient-a"),
                "support_path": expected_path_trace(support_path),
                "sqlite_support_path_text_lossy": expected_text_trace(&support_path.to_string_lossy()),
                "support_kind": expected_text_trace("dicom_study"),
                "extracted_path": null,
                "sqlite_extracted_path_text_lossy": null,
                "viewer_paths": [],
                "html_viewer_path": null,
                "warnings": [],
                "study_instance_uid": expected_text_trace("1.2.826.0.1.3680043.10.543.1"),
                "study_date": expected_text_trace("2026-01-02"),
                "study_description": expected_text_trace("Synthetic Study"),
                "instance_count": 2,
                "support_paths": [],
        }]
    })
}

fn assert_database_trace_calls(
    records: &[Value],
    function: &str,
    arguments: &Value,
    result: &Value,
    expected_count: Option<usize>,
) {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert!(!entries.is_empty(), "ingresso mancante: {function}");
    if let Some(expected_count) = expected_count {
        assert_eq!(entries.len(), expected_count, "numero ingressi: {function}");
    }
    for entry in entries {
        assert_eq!(entry["arguments"], *arguments, "argomenti: {function}");
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        let exits: Vec<_> = records
            .iter()
            .filter(|record| {
                record["type"] == "function_exit"
                    && record["function"] == function
                    && record["call_id"] == call_id
            })
            .collect();
        assert_eq!(exits.len(), 1, "uscita correlata per {function}#{call_id}");
        assert_eq!(exits[0]["outcome"], "returned", "esito: {function}");
        assert!(
            exits[0]["duration_ms"].is_u64(),
            "durata mancante: {function}#{call_id}"
        );
        assert_eq!(exits[0]["result"], *result, "risultato: {function}");
    }
}

#[test]
fn synthetic_database_trace_matches_complete_arguments_results_and_durations() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let document = document(directory.path());
    let metadata = CuratedMetadata::default();
    let dicom = study(directory.path());
    let extracted = ExtractedText {
        document_id: document.document_id.clone(),
        text: "api_key=SYNTHETIC-DATABASE-SECRET".into(),
        warnings: vec!["synthetic parser warning".into()],
        page_spans: vec![],
    };
    let extracted_text = [extracted];
    let documents = [document];
    let dicom_studies = [dicom];
    let logs = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione trace sintetica");

    let result = session
        .scoped(|| {
            build_database(
                &person,
                &documents,
                &metadata,
                &dicom_studies,
                &extracted_text,
            )
        })
        .expect("build database sintetico");
    drop(session);
    assert_eq!(result.documents, 1);
    assert_eq!(result.timeline_events, 0);
    assert_eq!(result.dicom_studies, 1);

    let log = fs::read_to_string(log_path).expect("log trace sintetico");
    assert!(!log.contains("SYNTHETIC-DATABASE-SECRET"));
    assert!(log.contains("secret_omitted"));
    let records: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
        .collect();
    let database = database_path(&person);
    let documents_trace = expected_document_trace(&documents[0].path);
    let studies_trace = expected_study_trace(&dicom_studies[0].support_path);
    let metadata_debug = concat!(
        "CuratedMetadata {\n",
        "    problems: [],\n",
        "    medications: [],\n",
        "    medication_leaflets: [],\n",
        "    medication_leaflet_exclusions: [],\n",
        "    therapies: [],\n",
        "    procedures: [],\n",
        "    observations: [],\n",
        "    observation_series: [],\n",
        "    observation_points: [],\n",
        "    timeline_events: [],\n",
        "    document_tags: {},\n",
        "    clinical_summary: None,\n",
        "}"
    );
    let extracted_debug_redacted = concat!(
        "[\n",
        "    ExtractedText {\n",
        "        document_id: \"doc-a\",\n",
        "        text: \"api_key=[secret_omitted]\",\n",
        "        warnings: [\n",
        "            \"synthetic parser warning\",\n",
        "        ],\n",
        "        page_spans: [],\n",
        "    },\n",
        "]"
    );
    let empty_result = expected_text_trace("()");

    assert_database_trace_calls(
        &records,
        "database::database_path",
        &expected_arguments(vec![(
            "local_build",
            expected_path_trace(&person.local_build),
        )]),
        &expected_path_trace(&database),
        Some(3),
    );
    let document_arguments = expected_arguments(vec![
        ("documents", documents_trace),
        (
            "extracted_text",
            expected_text_trace(extracted_debug_redacted),
        ),
        (
            "sqlite_connection",
            json!({"kind": "omitted", "reason": "external_component"}),
        ),
    ]);
    assert_database_trace_calls(
        &records,
        "database::insert_documents",
        &document_arguments,
        &empty_result,
        Some(1),
    );
    assert_database_trace_calls(
        &records,
        "database::insert_documents_impl",
        &document_arguments,
        &empty_result,
        Some(1),
    );
    let metadata_arguments = expected_arguments(vec![
        ("curated_metadata", expected_text_trace(metadata_debug)),
        (
            "sqlite_connection",
            json!({"kind": "omitted", "reason": "external_component"}),
        ),
    ]);
    assert_database_trace_calls(
        &records,
        "database::insert_metadata",
        &metadata_arguments,
        &empty_result,
        Some(1),
    );
    assert_database_trace_calls(
        &records,
        "database::insert_metadata_impl",
        &metadata_arguments,
        &empty_result,
        Some(1),
    );
    let dicom_arguments = expected_arguments(vec![
        ("dicom_studies", studies_trace),
        (
            "sqlite_connection",
            json!({"kind": "omitted", "reason": "external_component"}),
        ),
    ]);
    assert_database_trace_calls(
        &records,
        "database::insert_dicom",
        &dicom_arguments,
        &empty_result,
        Some(1),
    );
    assert_database_trace_calls(
        &records,
        "database::insert_dicom_impl",
        &dicom_arguments,
        &empty_result,
        Some(1),
    );
}

#[test]
fn builds_complete_archive_with_curated_rows_dicom_and_fts() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let document = document(directory.path());
    let metadata = metadata();
    let extracted = ExtractedText {
        document_id: document.document_id.clone(),
        text: "Synthetic extracted hemoglobin content".into(),
        warnings: vec![],
        page_spans: vec![],
    };
    let result = build_database(
        &person,
        std::slice::from_ref(&document),
        &metadata,
        &[study(directory.path())],
        &[extracted],
    )
    .expect("database sintetico completo");

    assert_eq!(result.path, database_path(&person));
    assert_eq!(result.documents, 1);
    assert_eq!(result.timeline_events, 1);
    assert_eq!(result.dicom_studies, 1);
    let connection = Connection::open(result.path).expect("database SQLite");
    let foreign_keys: i64 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("foreign key attive");
    let text: String = connection
        .query_row(
            "SELECT text FROM document_text WHERE document_id='doc-a'",
            [],
            |row| row.get(0),
        )
        .expect("testo estratto");
    let therapy: (String, String) = connection
        .query_row(
            "SELECT schedule, instructions FROM therapies WHERE id='therapy-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("terapia");
    let point: (String, i64, String, String) = connection
        .query_row("SELECT document_href, line_number, normalized_unit, reason_code FROM observation_points WHERE id='point-a'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .expect("provenienza osservazione");
    let dicom: (String, String, i64) = connection
        .query_row(
            "SELECT support_kind, study_instance_uid, instance_count FROM dicom_studies",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("metadati DICOM");
    let fts_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM document_fts WHERE document_fts MATCH 'hemoglobin'",
            [],
            |row| row.get(0),
        )
        .expect("query FTS5");
    let foreign_key_violations: i64 = connection
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("controllo foreign key");

    assert_eq!(foreign_keys, 1);
    assert_eq!(text, "Synthetic extracted hemoglobin content");
    assert_eq!(therapy, ("risveglio,cena".into(), "dopo il pasto".into()));
    assert_eq!(
        point,
        (
            "../documents/report.txt".into(),
            1,
            "g/dL".into(),
            "ACCEPTED_CONFIGURED_SYNONYM".into()
        )
    );
    assert_eq!(
        dicom,
        (
            "dicom_study".into(),
            "1.2.826.0.1.3680043.10.543.1".into(),
            2
        )
    );
    assert_eq!(fts_count, 1);
    assert_eq!(foreign_key_violations, 0);
}

#[test]
fn database_trace_records_sqlite_inputs_and_marks_runtime_boundary() {
    let directory = TestDirectory::new();
    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let mut config = person(directory.path());
        config.local_build = directory.path().join(format!("build-{suffix}"));
        let mut document = document(directory.path());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            document.path = directory
                .path()
                .join(std::ffi::OsString::from_vec(b"report-\xff.txt".to_vec()));
        }
        let extracted = ExtractedText {
            document_id: document.document_id.clone(),
            text: "Synthetic private extracted text".into(),
            warnings: vec!["Synthetic parser warning".into()],
            page_spans: vec![],
        };
        let metadata = metadata();
        let study = study(directory.path());
        let logs = directory.path().join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        let result = session.scoped(|| {
            build_database(
                &config,
                std::slice::from_ref(&document),
                &metadata,
                std::slice::from_ref(&study),
                std::slice::from_ref(&extracted),
            )
        });
        assert!(result.is_ok());
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("database::build_database"));
            assert!(log.contains("database::insert_documents"));
            assert!(log.contains("database::build_database_impl"));
            assert!(log.contains("database::insert_documents_impl"));
            assert!(log.contains("database::insert_metadata_impl"));
            assert!(log.contains("database::insert_dicom_impl"));
            assert!(log.contains("database::DatabaseStage::new"));
            assert!(log.contains("database::DatabaseStage::drop"));
            assert!(log.contains("database::insert_metadata"));
            assert!(log.contains("database::insert_dicom"));
            assert!(log.contains("sqlite_schema"));
            assert!(log.contains("sqlite_runtime_state"));
            assert!(log.contains("external_component"));
            assert!(log.contains("Synthetic private extracted text"));
            assert!(log.contains("1.2.826.0.1.3680043.10.543.1"));
            #[cfg(unix)]
            {
                assert!(log.contains("unix_bytes"));
                assert!(log.contains("sqlite_path_text_lossy"));
                assert!(log.contains("lossy_utf8"));
            }
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("Synthetic private extracted text"));
            assert!(!log.contains("1.2.826.0.1.3680043.10.543.1"));
        }
    }
}

#[test]
fn database_trace_redacts_known_secret_assignments_and_preserves_stored_text() {
    let directory = TestDirectory::new();
    let config = person(directory.path());
    let document = document(directory.path());
    let secret = "SECRET-SYNTHETIC-DATABASE";
    let text = format!("api_key={secret}");
    let extracted = ExtractedText {
        document_id: document.document_id.clone(),
        text: text.clone(),
        warnings: Vec::new(),
        page_spans: Vec::new(),
    };
    let logs = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione trace sintetica");

    let result = session.scoped(|| {
        build_database(
            &config,
            std::slice::from_ref(&document),
            &CuratedMetadata::default(),
            &[],
            &[extracted],
        )
    });
    let result = result.expect("database sintetico");
    drop(session);

    let connection = Connection::open(result.path).expect("database sintetico leggibile");
    let stored: String = connection
        .query_row(
            "SELECT text FROM document_text WHERE document_id = ?1",
            [&document.document_id],
            |row| row.get(0),
        )
        .expect("testo sintetico conservato nel database");
    assert_eq!(stored, text);

    let log = fs::read_to_string(log_path).expect("log trace sintetico");
    assert!(!log.contains(secret));
    assert!(log.contains("secret_omitted"));
    assert!(log.contains("database::build_database_impl"));
    assert!(log.contains("database::insert_documents_impl"));
}

#[test]
fn failed_transaction_preserves_the_previous_database() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let document = document(directory.path());
    let valid = CuratedMetadata::default();
    build_database(&person, std::slice::from_ref(&document), &valid, &[], &[])
        .expect("prima build");
    let invalid = CuratedMetadata {
        therapies: vec![TherapyEpisode {
            id: "invalid-therapy".into(),
            medication_id: "missing-drug".into(),
            start_date: None,
            end_date: None,
            dosage: None,
            role: None,
            schedule: vec![],
            instructions: None,
        }],
        ..CuratedMetadata::default()
    };

    assert!(build_database(&person, std::slice::from_ref(&document), &invalid, &[], &[]).is_err());

    let connection = Connection::open(database_path(&person)).expect("database precedente intatto");
    let count: i64 = connection
        .query_row("SELECT count(*) FROM documents", [], |row| row.get(0))
        .expect("riga precedente leggibile");
    assert_eq!(count, 1);
}

#[test]
fn duplicate_document_ids_report_both_source_locations() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let source = document(directory.path());
    let mut staged = source.clone();
    staged.path = directory
        .path()
        .join("generated/staging/containers/report.txt");
    staged.origin = "container".into();
    staged.container_id = Some("synthetic-container-id".into());
    staged.internal_path = Some("README/report.txt".into());

    let error = build_database(
        &person,
        &[source.clone(), staged.clone()],
        &CuratedMetadata::default(),
        &[],
        &[],
    )
    .expect_err("ID duplicato rifiutato prima dell'inserimento SQLite");

    assert!(error.message().contains(&source.path.display().to_string()));
    assert!(error.message().contains(&staged.path.display().to_string()));
    assert!(error.message().contains("README/report.txt"));
    assert!(error.message().contains("synthetic-container-id"));
}
