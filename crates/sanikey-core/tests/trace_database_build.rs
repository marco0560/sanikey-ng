//! Verifica gli argomenti e i risultati Trace della build SQLite minima.

use sanikey_core::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use sanikey_core::database::{DatabaseBuildResult, build_database};
use sanikey_core::logging::{Component, Level, Session};
use sanikey_core::metadata::CuratedMetadata;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Creates the synthetic workspace used by this integration test.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-database-build-{}-{}",
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

/// Removes the synthetic workspace when the test finishes.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn path_trace(path: &Path) -> Value {
    json!({"kind":"data", "value":{"encoding":"utf8", "value":path.to_str().unwrap()}})
}

fn text_trace(value: &str) -> Value {
    json!({
        "kind":"data",
        "value":{"format":"text_redacted_known_secret_assignments", "value":value}
    })
}

fn argument(name: &str, content: Value) -> Value {
    json!({"name":name, "content":content})
}

fn expected_arguments(person: &PersonConfig, metadata: &CuratedMetadata) -> Value {
    let path = person
        .local_build
        .join("database")
        .join("medical_archive.db");
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
    assert_eq!(format!("{metadata:#?}"), metadata_debug);
    json!([
        argument("person_id", text_trace("person-synthetic")),
        argument("database_path", path_trace(&path)),
        argument("documents", json!({"kind":"data", "value":[]})),
        argument("curated_metadata", text_trace(metadata_debug)),
        argument("dicom_studies", json!({"kind":"data", "value":[]})),
        argument("extracted_text", text_trace("[]")),
        argument(
            "sqlite_schema",
            json!({"kind":"data", "value":EXPECTED_SCHEMA})
        ),
        argument(
            "sqlite_runtime_state",
            json!({"kind":"omitted", "reason":"external_component"})
        )
    ])
}

const EXPECTED_SCHEMA: &str = r#"
        CREATE TABLE documents (
            id TEXT PRIMARY KEY,
            patient_id TEXT NOT NULL,
            path TEXT NOT NULL,
            title TEXT NOT NULL,
            category TEXT NOT NULL,
            kind TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            document_date TEXT,
            series TEXT,
            origin TEXT NOT NULL,
            container_id TEXT,
            internal_path TEXT
        );

        CREATE VIRTUAL TABLE document_fts USING fts5(
            title,
            category,
            tags,
            text
        );

        CREATE TABLE document_text (
            document_id TEXT PRIMARY KEY,
            text TEXT NOT NULL,
            FOREIGN KEY (document_id) REFERENCES documents(id)
        );

        CREATE TABLE problems (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            status TEXT NOT NULL
        );

        CREATE TABLE medications (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            active_ingredient TEXT,
            form TEXT,
            strength_per_unit TEXT
        );

        CREATE TABLE therapies (
            id TEXT PRIMARY KEY,
            medication_id TEXT NOT NULL,
            start_date TEXT,
            end_date TEXT,
            dosage TEXT,
            role TEXT,
            schedule TEXT NOT NULL,
            instructions TEXT,
            FOREIGN KEY (medication_id) REFERENCES medications(id)
        );

        CREATE TABLE procedures (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            procedure_date TEXT,
            status TEXT NOT NULL
        );

        CREATE TABLE observations (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            value TEXT NOT NULL,
            observation_date TEXT
        );

        CREATE TABLE observation_series (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            value_type TEXT NOT NULL,
            unit TEXT,
            description TEXT,
            warn_duplicate_same_day INTEGER NOT NULL,
            synonyms TEXT NOT NULL,
            parameter_rule_id TEXT,
            parameter_rule_version INTEGER,
            parameter_rule_digest TEXT,
            unit_variant TEXT
        );

        CREATE TABLE observation_points (
            id TEXT PRIMARY KEY,
            series_id TEXT NOT NULL,
            observation_date TEXT NOT NULL,
            source_type TEXT NOT NULL,
            source_reference TEXT NOT NULL,
            numeric_value REAL,
            text_value TEXT,
            systolic REAL,
            diastolic REAL,
            pulse REAL,
            note TEXT,
            source_kind TEXT NOT NULL,
            document_id TEXT,
            document_href TEXT,
            document_title TEXT,
            document_category TEXT,
            source_text_digest TEXT,
            original_line TEXT,
            line_number INTEGER,
            page_number INTEGER,
            character_start INTEGER,
            character_end INTEGER,
            matched_label TEXT,
            raw_value TEXT,
            parsed_value REAL,
            raw_unit TEXT,
            normalized_unit TEXT,
            qualifier TEXT,
            rule_id TEXT,
            rule_version INTEGER,
            rule_digest TEXT,
            reason_code TEXT,
            FOREIGN KEY (series_id) REFERENCES observation_series(id)
        );

        CREATE TABLE timeline_events (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            start_date TEXT,
            end_date TEXT,
            source TEXT NOT NULL,
            links TEXT NOT NULL
        );

        CREATE TABLE dicom_studies (
            id TEXT PRIMARY KEY,
            patient_id TEXT NOT NULL,
            support_path TEXT NOT NULL,
            support_kind TEXT NOT NULL,
            extracted_path TEXT,
            viewer_count INTEGER NOT NULL,
            warning_count INTEGER NOT NULL,
            study_instance_uid TEXT,
            study_date TEXT,
            study_description TEXT,
            instance_count INTEGER NOT NULL
        );
"#;

fn expected_result(path: &Path) -> Value {
    json!({
        "kind":"data",
        "value":[
            {"name":"state", "content":{"kind":"data", "value":"succeeded"}},
            {"name":"database_path", "content":path_trace(path)},
            {"name":"documents", "content":{"kind":"data", "value":0}},
            {"name":"timeline_events", "content":{"kind":"data", "value":0}},
            {"name":"dicom_studies", "content":{"kind":"data", "value":0}}
        ]
    })
}

fn assert_span(
    records: &[Value],
    function: &str,
    expected_parent: Option<u64>,
    args: &Value,
    result: &Value,
) -> u64 {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_enter"
                && record["function"] == function
                && record["parent_call_id"].as_u64() == expected_parent
        })
        .collect();
    assert_eq!(entries.len(), 1, "un ingresso {function}");
    let entry = entries[0];
    assert_eq!(entry["component"], "core");
    assert_eq!(entry["arguments"], *args);
    let call_id = entry["call_id"].as_u64().expect("call_id numerico");
    let exits: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_exit"
                && record["function"] == function
                && record["call_id"] == call_id
        })
        .collect();
    assert_eq!(
        exits.len(),
        1,
        "una uscita correlata per {function}#{call_id}"
    );
    assert_eq!(exits[0]["outcome"], "returned");
    assert!(exits[0]["duration_ms"].as_u64().is_some());
    assert_eq!(exits[0]["result"], *result);
    call_id
}

#[test]
fn database_build_wrapper_and_implementation_trace_full_inputs_outputs_and_parentage() {
    let directory = TestDirectory::new();
    let person = PersonConfig {
        id: "person-synthetic".to_owned(),
        display_name: "Persona sintetica".to_owned(),
        source_documents: directory.path().join("documents"),
        metadata_directory: directory.path().join("metadata"),
        local_build: directory.path().join("build"),
        usb_uuid: "UUID-SYNTHETIC".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    };
    let metadata = CuratedMetadata::default();
    let documents = [];
    let dicom_studies = [];
    let extracted_text = [];
    let database_path = person
        .local_build
        .join("database")
        .join("medical_archive.db");
    let args = expected_arguments(&person, &metadata);
    let output = expected_result(&database_path);
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");
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
        .expect("build SQLite sintetica");
    assert_eq!(
        result,
        DatabaseBuildResult {
            path: database_path.clone(),
            documents: 0,
            timeline_events: 0,
            dicom_studies: 0,
        }
    );
    assert!(database_path.is_file());
    session.finish(Component::Core).expect("chiusura Trace");
    drop(session);

    let records: Vec<Value> = fs::read_to_string(log_path)
        .expect("log Trace sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect();
    let wrapper_id = assert_span(&records, "database::build_database", None, &args, &output);
    let implementation_id = assert_span(
        &records,
        "database::build_database_impl",
        Some(wrapper_id),
        &args,
        &output,
    );
    assert_ne!(wrapper_id, implementation_id);
}
