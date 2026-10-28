use super::{build_database, database_path};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::dicom::DicomStudy;
use crate::metadata::{
    ClinicalProblem, CuratedMetadata, DocumentRecord, Medication, Observation, ObservationPoint,
    ObservationSeries, Procedure, TherapyEpisode, TimelineEvent,
};
use crate::pdf::ExtractedText;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

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
