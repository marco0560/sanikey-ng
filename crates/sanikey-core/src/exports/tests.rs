//! Fixture sintetiche per la generazione degli export.

use super::{generate_exports, render_markdown, write_json, write_script};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, SearchDictionary, UiConfig};
use crate::dicom::DicomStudy;
use crate::logging::{Component, Level, Session};
use crate::metadata::{
    ClinicalProblem, CuratedMetadata, DocumentRecord, Medication, MedicationLeaflet,
    MedicationLeafletExclusion, Observation, ObservationPoint, ObservationSeries, Procedure,
    TherapyEpisode,
};
use crate::operation::{ErrorCode, OperationError};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Gestisce le directory temporanee create dai test export.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-exports-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Rimuove i file generati dalle fixture.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(root: &Path, id: &str) -> PersonConfig {
    PersonConfig {
        id: id.to_owned(),
        display_name: format!("Paziente sintetico {id}"),
        source_documents: root.join("source"),
        metadata_directory: root.join("metadata"),
        local_build: root.join("build"),
        usb_uuid: "fixture-volume".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn document(person: &PersonConfig, name: &str) -> DocumentRecord {
    let path = person.source_documents.join(name);
    fs::create_dir_all(path.parent().expect("parent documento")).expect("directory documento");
    fs::write(&path, "fixture sintetica").expect("documento sintetico");
    DocumentRecord {
        document_id: format!("sha256-{name}"),
        patient_id: person.id.clone(),
        path,
        title: "Referto sintetico".to_owned(),
        category: "visite".to_owned(),
        kind: "txt".to_owned(),
        sha256: "sintetico".to_owned(),
        date: Some("2026-01-02".to_owned()),
        series: None,
        tags: vec!["clinica".to_owned()],
        origin: "source".to_owned(),
        container_id: None,
        internal_path: None,
    }
}

#[test]
fn exports_clinical_search_timeline_and_file_url_javascript() {
    let directory = TestDirectory::new();
    let person = person(directory.path(), "patient-a");
    let document = document(&person, "20260102 Referto.txt");
    let mut metadata = CuratedMetadata::default();
    metadata.problems.push(ClinicalProblem {
        id: "problem-a".to_owned(),
        title: "Ipertensione".to_owned(),
        status: "active".to_owned(),
        provenance: Vec::new(),
    });
    metadata.medications.push(Medication {
        id: "drug-a".to_owned(),
        name: "Farmaco sintetico".to_owned(),
        active_ingredient: Some("Principio attivo".to_owned()),
        form: Some("compresse".to_owned()),
        strength_per_unit: Some("10 mg".to_owned()),
    });
    metadata.medication_leaflets.push(MedicationLeaflet {
        medication_id: "drug-a".to_owned(),
        codice_sis: "123".to_owned(),
        aic6: "456".to_owned(),
        downloaded_at: Some("2026-07-18".to_owned()),
        source_fingerprint: None,
    });
    metadata
        .medication_leaflet_exclusions
        .push(MedicationLeafletExclusion {
            medication_id: "drug-a".to_owned(),
            reason: "non_aifa".to_owned(),
        });
    metadata.therapies.push(TherapyEpisode {
        id: "therapy-a".to_owned(),
        medication_id: "drug-a".to_owned(),
        start_date: Some("2026-01-03".to_owned()),
        end_date: Some("2026-01-31".to_owned()),
        dosage: Some("1 compressa".to_owned()),
        role: None,
        schedule: vec!["mattina".to_owned(), "sera".to_owned()],
        instructions: None,
    });
    let leaflet_directory = person.local_build.join("medication-leaflets/drug-a");
    fs::create_dir_all(&leaflet_directory).expect("directory fogli AIFA");
    fs::write(
        leaflet_directory.join("foglio-illustrativo.pdf"),
        b"%PDF-fi",
    )
    .expect("foglio illustrativo sintetico");
    fs::write(leaflet_directory.join("rcp.pdf"), b"%PDF-rcp").expect("RCP sintetico");

    let result =
        generate_exports(&person, &[document], &metadata, &[], &[]).expect("export sintetici");
    let search: Value = serde_json::from_slice(&fs::read(&result.search).expect("search JSON"))
        .expect("JSON valido");
    let timeline: Value =
        serde_json::from_slice(&fs::read(&result.timeline).expect("timeline JSON"))
            .expect("JSON valido");
    let summary: Value = serde_json::from_slice(&fs::read(&result.summary).expect("summary JSON"))
        .expect("JSON valido");
    let script = fs::read_to_string(&result.data_script).expect("script dati");
    let payload: Value = serde_json::from_str(
        script
            .strip_prefix("window.SANIKEY_DATA = ")
            .and_then(|value| value.strip_suffix(";\n"))
            .expect("assegnazione JavaScript attesa"),
    )
    .expect("JSON incorporato valido");

    let therapy = &payload["clinical"]["therapies"][0];
    assert_eq!(therapy["title"], "Terapia: Farmaco sintetico");
    assert_eq!(therapy["schedule"], serde_json::json!(["mattina", "sera"]));
    assert_eq!(
        therapy["leaflet_href"],
        "../medication-leaflets/drug-a/foglio-illustrativo.pdf"
    );
    assert_eq!(therapy["rcp_href"], "../medication-leaflets/drug-a/rcp.pdf");
    assert_eq!(
        therapy["aifa_fi_url"],
        "https://api.aifa.gov.it/aifa-bdf-eif-be/1.0.0/organizzazione/123/farmaci/456/stampati?ts=FI"
    );
    assert_eq!(therapy["non_aifa"], true);
    assert!(
        search
            .as_array()
            .expect("array ricerca")
            .iter()
            .any(|item| { item["type"] == "problem" && item["text"] == "Ipertensione active" })
    );
    assert!(
        timeline
            .as_array()
            .expect("array cronologia")
            .iter()
            .any(|item| { item["id"] == "therapy-therapy-a" && item["end_date"] == "2026-01-31" })
    );
    assert_eq!(summary["patient_id"], "patient-a");
    assert!(!script.contains(directory.path().to_string_lossy().as_ref()));
    assert!(result.content_search_script.is_file());
}

#[test]
fn exports_trace_captures_private_payloads_and_native_paths_only_at_trace_level() {
    let directory = TestDirectory::new();
    let person = person(directory.path(), "patient-trace");
    let mut document = document(&person, "initial.md");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        document.path = person
            .source_documents
            .join(std::ffi::OsString::from_vec(b"report-\xff.md".to_vec()));
    }
    fs::write(
        &document.path,
        "password = synthetic-markdown-source-secret; private Markdown detail",
    )
    .expect("sorgente Markdown sintetica");
    let mut metadata = CuratedMetadata {
        clinical_summary: Some("Synthetic private clinical summary".into()),
        ..CuratedMetadata::default()
    };
    metadata.medications.push(Medication {
        id: "synthetic-drug".into(),
        name: "Private medication detail".into(),
        active_ingredient: Some("Synthetic ingredient".into()),
        form: None,
        strength_per_unit: None,
    });
    metadata.medication_leaflets.push(MedicationLeaflet {
        medication_id: "synthetic-drug".into(),
        codice_sis: "fixture-sis".into(),
        aic6: "fixture-aic".into(),
        downloaded_at: Some("2026-02-03".into()),
        source_fingerprint: None,
    });

    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let mut config = person.clone();
        config.local_build = directory.path().join(format!("build-{suffix}"));
        let logs = directory.path().join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        let extracted = crate::pdf::ExtractedText {
            document_id: document.document_id.clone(),
            text: "Synthetic private extracted source text".into(),
            warnings: vec![],
            page_spans: vec![],
        };
        let result = session.scoped(|| {
            let unsafe_url =
                super::unsafe_markdown_url("javascript:api_key = synthetic-url-secret", false);
            assert!(unsafe_url);
            let error = super::export_error(
                "password = synthetic-export-error-secret; private error detail".to_owned(),
            );
            assert_eq!(error.code(), ErrorCode::Configuration);
            assert!(error.message().contains("private error detail"));
            generate_exports(
                &config,
                std::slice::from_ref(&document),
                &metadata,
                &[],
                std::slice::from_ref(&extracted),
            )
        });
        assert!(result.is_ok());
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("exports::generate_exports"));
            assert!(log.contains("exports::write_bytes"));
            assert!(log.contains("exports::generate_exports_impl"));
            assert!(log.contains("exports::document_payload_impl"));
            assert!(log.contains("exports::clinical_payload_impl"));
            assert!(log.contains("exports::render_markdown_impl"));
            assert!(log.contains("exports::write_json_impl"));
            assert!(log.contains("exports::write_script_impl"));
            assert!(log.contains("exports::write_bytes_impl"));
            assert!(log.contains("exports::document_search_payload"));
            assert!(log.contains("exports::document_search_payload_impl"));
            assert!(log.contains("exports::metadata_search_payloads"));
            assert!(log.contains("exports::metadata_search_payloads_impl"));
            assert!(log.contains("exports::timeline_payload"));
            assert!(log.contains("exports::timeline_payload_impl"));
            assert!(log.contains("exports::write_technical_documents"));
            assert!(log.contains("exports::write_technical_documents_impl"));
            assert!(log.contains("exports::write_dicom_viewer_manifest"));
            assert!(log.contains("exports::write_dicom_viewer_manifest_impl"));
            assert!(log.contains("exports::resolved_tags"));
            assert!(log.contains("exports::consultation_href"));
            assert!(log.contains("exports::source_href"));
            assert!(log.contains("exports::source_href_from_path"));
            assert!(log.contains("exports::display_path"));
            assert!(log.contains("exports::join_present"));
            assert!(log.contains("exports::visible_fields"));
            assert!(log.contains("exports::entity_subtitle"));
            assert!(log.contains("exports::download_date"));
            assert!(log.contains("exports::download_date_impl"));
            assert!(log.contains("exports::aifa_url"));
            assert!(log.contains("exports::markdown_summary"));
            assert!(log.contains("exports::unsafe_markdown_url"));
            assert!(log.contains("exports::escape_html"));
            assert!(log.contains("exports::serialize_python_json"));
            assert!(log.contains("exports::export_error"));
            assert!(log.contains("Synthetic private clinical summary"));
            assert!(log.contains("Synthetic private extracted source text"));
            assert!(log.contains("private Markdown detail"));
            assert!(log.contains("private error detail"));
            assert!(log.contains("secret_omitted"));
            assert!(!log.contains("synthetic-markdown-source-secret"));
            assert!(!log.contains("synthetic-url-secret"));
            assert!(!log.contains("synthetic-export-error-secret"));
            #[cfg(unix)]
            assert!(log.contains("unix_bytes"));
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("Synthetic private clinical summary"));
            assert!(!log.contains("Synthetic private extracted source text"));
            assert!(!log.contains("private Markdown detail"));
            assert!(!log.contains("private error detail"));
            assert!(!log.contains("synthetic-markdown-source-secret"));
            assert!(!log.contains("synthetic-url-secret"));
            assert!(!log.contains("synthetic-export-error-secret"));
            assert!(!log.contains("unix_bytes"));
        }
    }

    let error = OperationError::new(
        ErrorCode::Configuration,
        "password = synthetic-export-secret",
    );
    let projected = super::trace_result::<()>(&Err(error)).to_json().to_string();
    assert!(projected.contains("secret_omitted"));
    assert!(!projected.contains("synthetic-export-secret"));
}

#[test]
fn export_trace_redacts_secret_keys_and_assignments_without_changing_files() {
    let directory = TestDirectory::new();
    let value = serde_json::json!({
        "ordinary_private_data": "private export detail",
        "nested": {"API_Key": "synthetic-json-secret"},
        "note": "password = synthetic-inline-secret; clinical detail retained",
    });

    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let logs = directory.path().join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        let json_path = directory.path().join(format!("export-{suffix}.json"));
        let script_path = directory.path().join(format!("export-{suffix}.js"));
        session.scoped(|| {
            write_json(&json_path, &value).expect("scrittura JSON sintetica");
            write_script(&script_path, "SyntheticExport", &value)
                .expect("scrittura script sintetico");
            assert!(
                render_markdown("password = synthetic-markdown-secret; dettaglio privato")
                    .contains("synthetic-markdown-secret")
            );
        });
        assert!(
            fs::read_to_string(&json_path)
                .expect("export JSON")
                .contains("synthetic-json-secret")
        );
        assert!(
            fs::read_to_string(&script_path)
                .expect("export JS")
                .contains("synthetic-inline-secret")
        );
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("secret_omitted"));
            assert!(log.contains("private export detail"));
            assert!(log.contains("clinical detail retained"));
            assert!(log.contains("exports::serialize_python_json::serialize"));
            assert!(!log.contains("synthetic-json-secret"));
            assert!(!log.contains("synthetic-inline-secret"));
            assert!(!log.contains("synthetic-markdown-secret"));
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("private export detail"));
            assert!(!log.contains("synthetic-json-secret"));
            assert!(!log.contains("synthetic-inline-secret"));
            assert!(!log.contains("synthetic-markdown-secret"));
        }
    }
}

#[test]
fn exports_for_two_people_use_disjoint_build_roots() {
    let directory = TestDirectory::new();
    let first = person(&directory.path().join("first"), "patient-a");
    let second = person(&directory.path().join("second"), "patient-b");
    let first_document = document(&first, "a.txt");
    let second_document = document(&second, "b.txt");
    let first_study = DicomStudy {
        study_id: "patient-a-study".to_owned(),
        patient_id: first.id.clone(),
        support_path: first_document.path.clone(),
        support_kind: "dicom_study".to_owned(),
        extracted_path: None,
        viewer_paths: Vec::new(),
        html_viewer_path: None,
        warnings: Vec::new(),
        study_instance_uid: Some("1.2.3".to_owned()),
        study_date: None,
        study_description: None,
        instance_count: 1,
        support_paths: vec![first_document.path.clone()],
    };
    let second_study = DicomStudy {
        study_id: "patient-b-study".to_owned(),
        patient_id: second.id.clone(),
        support_path: second_document.path.clone(),
        support_kind: "dicom_study".to_owned(),
        extracted_path: None,
        viewer_paths: Vec::new(),
        html_viewer_path: None,
        warnings: Vec::new(),
        study_instance_uid: Some("4.5.6".to_owned()),
        study_date: None,
        study_description: None,
        instance_count: 1,
        support_paths: vec![second_document.path.clone()],
    };

    let both_documents = [first_document, second_document];
    let both_studies = [first_study, second_study];
    let first_result = generate_exports(
        &first,
        &both_documents,
        &CuratedMetadata::default(),
        &both_studies,
        &[],
    )
    .expect("export prima persona");
    let second_result = generate_exports(
        &second,
        &both_documents,
        &CuratedMetadata::default(),
        &both_studies,
        &[],
    )
    .expect("export seconda persona");

    let first_payload: Value =
        serde_json::from_slice(&fs::read(first_result.documents).expect("primo JSON"))
            .expect("primo JSON valido");
    let second_payload: Value =
        serde_json::from_slice(&fs::read(second_result.documents).expect("secondo JSON"))
            .expect("secondo JSON valido");
    let first_script = fs::read_to_string(first_result.data_script).expect("primo script");
    let second_script = fs::read_to_string(second_result.data_script).expect("secondo script");
    let first_clinical: Value = serde_json::from_str(
        first_script
            .strip_prefix("window.SANIKEY_DATA = ")
            .and_then(|value| value.strip_suffix(";\n"))
            .expect("prima assegnazione JavaScript"),
    )
    .expect("primo payload JSON");
    let second_clinical: Value = serde_json::from_str(
        second_script
            .strip_prefix("window.SANIKEY_DATA = ")
            .and_then(|value| value.strip_suffix(";\n"))
            .expect("seconda assegnazione JavaScript"),
    )
    .expect("secondo payload JSON");
    assert_eq!(first_payload[0]["id"], "sha256-a.txt");
    assert_eq!(second_payload[0]["id"], "sha256-b.txt");
    assert_eq!(
        first_clinical["clinical"]["dicom_studies"][0]["id"],
        "patient-a-study"
    );
    assert_eq!(
        second_clinical["clinical"]["dicom_studies"][0]["id"],
        "patient-b-study"
    );
    assert_ne!(first_result.data_dir, second_result.data_dir);
}

#[test]
fn exports_advanced_search_dictionary_and_size_warning() {
    let directory = TestDirectory::new();
    let mut person = person(directory.path(), "patient-a");
    person.search.dictionary_data = SearchDictionary {
        terms: [("rx".to_owned(), vec!["radiografia".to_owned()])]
            .into_iter()
            .collect(),
        months: [("marzo".to_owned(), vec!["03".to_owned(), "3".to_owned()])]
            .into_iter()
            .collect(),
    };
    person.search.advanced_index_warning_mb = 0;
    let document = document(&person, "20260102 Report.txt");
    let extracted = crate::pdf::ExtractedText {
        document_id: document.document_id.clone(),
        text: "Creatinina e radiografia torace".to_owned(),
        warnings: Vec::new(),
        page_spans: Vec::new(),
    };

    let result = generate_exports(
        &person,
        &[document],
        &CuratedMetadata::default(),
        &[],
        &[extracted],
    )
    .expect("export ricerca avanzata");
    let script = fs::read_to_string(&result.content_search_script).expect("script ricerca");

    assert!(script.contains("\"rx\": [\"radiografia\"]"));
    assert!(script.contains("\"marzo\": [\"03\", \"3\"]"));
    assert!(script.contains("Creatinina e radiografia torace"));
    assert_eq!(result.warning_messages.len(), 1);
}

#[test]
fn exports_dicom_viewer_links_and_manifest_for_a_synthetic_study() {
    use crate::logging::{Component, Level, Session};

    let directory = TestDirectory::new();
    let person = person(directory.path(), "patient-a");
    let support = document(&person, "20260102 TAC.zip");
    let extracted = person.local_build.join("staging/containers/container-a");
    let viewer = extracted.join("IHE_PDI/PAGES/STUDIES/STUDY1.HTM");
    fs::create_dir_all(viewer.parent().expect("directory viewer")).expect("directory viewer");
    fs::write(&viewer, "<html>fixture</html>").expect("viewer sintetico");
    let study = DicomStudy {
        study_id: "study-a".to_owned(),
        patient_id: person.id.clone(),
        support_path: support.path.clone(),
        support_kind: "dicom_zip".to_owned(),
        extracted_path: Some(extracted.clone()),
        viewer_paths: vec![viewer.clone()],
        html_viewer_path: Some(viewer),
        warnings: Vec::new(),
        study_instance_uid: Some("1.2.3".to_owned()),
        study_date: Some("2026-01-02".to_owned()),
        study_description: Some("TAC sintetica".to_owned()),
        instance_count: 4,
        support_paths: vec![support.path.clone()],
    };

    let logs = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione trace DICOM");
    let result = session
        .scoped(|| {
            assert!(
                super::dicom_studies_for_document(&support, std::slice::from_ref(&study)).is_some()
            );
            generate_exports(
                &person,
                &[support],
                &CuratedMetadata::default(),
                &[study],
                &[],
            )
        })
        .expect("export DICOM");
    drop(session);
    let log = fs::read_to_string(log_path).expect("log DICOM");
    let search: Value = serde_json::from_slice(&fs::read(result.search).expect("search JSON"))
        .expect("search valido");
    let manifest: Value = serde_json::from_slice(
        &fs::read(person.local_build.join("manifests/dicom_html_viewers.json"))
            .expect("manifest viewer"),
    )
    .expect("manifest valido");
    let expected_href = "../dicom-viewers/study-a/IHE_PDI/PAGES/STUDIES/STUDY1.HTM";

    assert_eq!(search[0]["viewer_href"], expected_href);
    assert!(log.contains("exports::consultation_href_impl"));
    assert!(log.contains("exports::dicom_studies_for_document"));
    assert!(log.contains("exports::dicom_viewer_href"));
    assert!(log.contains("exports::dicom_preview_href_impl"));
    assert!(log.contains("exports::dicom_media_href"));
    assert!(log.contains("exports::dicom_media_href_impl"));
    assert_eq!(manifest["viewers"][0]["relative_root"], "IHE_PDI");
    assert_eq!(
        manifest["viewers"][0]["entrypoint"],
        "IHE_PDI/PAGES/STUDIES/STUDY1.HTM"
    );
}

#[test]
fn exports_technical_documents_with_quoted_csv_and_escaped_html() {
    let directory = TestDirectory::new();
    let person = person(directory.path(), "patient-a");
    let mut record = document(&person, "non <consultabile>.zip");
    record.kind = "archive".to_owned();
    record.title = "Archivio <sintetico>".to_owned();

    let result = generate_exports(&person, &[record], &CuratedMetadata::default(), &[], &[])
        .expect("export tecnico");
    let csv = fs::read_to_string(
        person
            .local_build
            .join("technical/documenti-non-apribili.csv"),
    )
    .expect("CSV tecnico");
    let html = fs::read_to_string(
        person
            .local_build
            .join("technical/documenti-non-apribili.html"),
    )
    .expect("HTML tecnico");
    let search: Value = serde_json::from_slice(&fs::read(result.search).expect("search JSON"))
        .expect("search valido");

    assert!(csv.contains("\"estensione\";\"nome\";\"origine\""));
    assert!(csv.contains("\".zip\";\"non <consultabile>.zip\""));
    assert!(html.contains("non &lt;consultabile&gt;.zip"));
    assert_eq!(search, serde_json::json!([]));
}

#[test]
fn exports_longitudinal_observation_series_and_missing_values_without_zeroes() {
    use crate::logging::{Component, Level, Session};

    let directory = TestDirectory::new();
    let person = person(directory.path(), "patient-a");
    let mut metadata = CuratedMetadata::default();
    metadata.observation_series.push(ObservationSeries {
        id: "pressure".to_owned(),
        name: "Pressione arteriosa".to_owned(),
        value_type: "blood_pressure".to_owned(),
        unit: Some("mmHg".to_owned()),
        description: None,
        warn_duplicate_same_day: true,
        synonyms: vec!["pressione".to_owned()],
        parameter_rule_id: None,
        parameter_rule_version: None,
        parameter_rule_digest: None,
        unit_variant: None,
    });
    metadata.observation_points.push(ObservationPoint {
        id: "pressure-2026-01-02".to_owned(),
        series_id: "pressure".to_owned(),
        observation_date: "2026-01-02".to_owned(),
        source_type: "csv".to_owned(),
        source_reference: "pressure.csv:2".to_owned(),
        numeric_value: None,
        text_value: None,
        systolic: Some(120.0),
        diastolic: Some(80.0),
        pulse: Some(60.0),
        note: None,
        source_kind: "csv".to_owned(),
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
    });

    let logs = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");
    let result = session
        .scoped(|| generate_exports(&person, &[], &metadata, &[], &[]))
        .expect("export parametri sintetici");
    drop(session);
    let script = fs::read_to_string(result.data_script).expect("script dati");
    let payload: Value = serde_json::from_str(
        script
            .strip_prefix("window.SANIKEY_DATA = ")
            .and_then(|value| value.strip_suffix(";\n"))
            .expect("assegnazione JavaScript attesa"),
    )
    .expect("JSON incorporato valido");

    let point = &payload["clinical"]["observation_points"][0];
    assert_eq!(point["title"], "Pressione arteriosa");
    assert_eq!(point["value"], "120/80 FC 60");
    assert!(point["numeric_value"].is_null());
    assert!(
        point["text"]
            .as_str()
            .unwrap_or_default()
            .contains("120/80 FC 60")
    );
    let log = fs::read_to_string(log_path).expect("log osservazioni");
    assert!(log.contains("exports::point_value"));
    assert!(log.contains("exports::format_number"));
}

#[test]
fn exports_timeline_oldest_first_when_requested() {
    let directory = TestDirectory::new();
    let mut person = person(directory.path(), "patient-a");
    person.ui.timeline_order = "asc".to_owned();
    let older = document(&person, "20260102 Older.txt");
    let mut newer = document(&person, "20260103 Newer.txt");
    newer.date = Some("2026-01-03".to_owned());

    let result = generate_exports(
        &person,
        &[newer, older],
        &CuratedMetadata::default(),
        &[],
        &[],
    )
    .expect("export cronologia ascendente");
    let timeline: Value =
        serde_json::from_slice(&fs::read(result.timeline).expect("timeline JSON"))
            .expect("timeline valido");

    assert_eq!(
        timeline
            .as_array()
            .expect("eventi timeline")
            .iter()
            .map(|event| event["start_date"].as_str().expect("data").to_owned())
            .collect::<Vec<_>>(),
        ["2026-01-02", "2026-01-03"]
    );
}

#[test]
fn exports_clinical_procedures_observations_and_hides_technical_dicom_documents() {
    let directory = TestDirectory::new();
    let person = person(directory.path(), "patient-a");
    let mut support = document(&person, "20260102 TAC.zip");
    support.kind = "archive".to_owned();
    let mut dicom_file = document(&person, "20260102 image.dcm");
    dicom_file.kind = "dicom_file".to_owned();
    let report = document(&person, "20260103 Report.txt");

    let mut metadata = CuratedMetadata::default();
    metadata.procedures.push(Procedure {
        id: "procedure-a".to_owned(),
        title: "Procedura sintetica".to_owned(),
        date: Some("2026-01-04".to_owned()),
        status: "completed".to_owned(),
    });
    metadata.observations.push(Observation {
        id: "observation-a".to_owned(),
        kind: "peso".to_owned(),
        value: "70 kg".to_owned(),
        date: Some("2026-01-05".to_owned()),
    });
    let study = DicomStudy {
        study_id: "study-no-viewer".to_owned(),
        patient_id: person.id.clone(),
        support_path: support.path.clone(),
        support_kind: "dicom_zip".to_owned(),
        extracted_path: None,
        viewer_paths: Vec::new(),
        html_viewer_path: None,
        warnings: Vec::new(),
        study_instance_uid: Some("1.2.3".to_owned()),
        study_date: Some("2026-01-02".to_owned()),
        study_description: Some("TAC torace".to_owned()),
        instance_count: 42,
        support_paths: vec![support.path.clone()],
    };

    let result = generate_exports(
        &person,
        &[support, dicom_file, report],
        &metadata,
        &[study],
        &[],
    )
    .expect("export clinico e DICOM sintetico");
    let documents: Value =
        serde_json::from_slice(&fs::read(result.documents).expect("documenti JSON"))
            .expect("documenti validi");
    let search: Value = serde_json::from_slice(&fs::read(result.search).expect("search JSON"))
        .expect("search valido");
    let script = fs::read_to_string(result.data_script).expect("script dati");
    let payload: Value = serde_json::from_str(
        script
            .strip_prefix("window.SANIKEY_DATA = ")
            .and_then(|value| value.strip_suffix(";\n"))
            .expect("assegnazione JavaScript attesa"),
    )
    .expect("payload JSON valido");

    assert_eq!(documents.as_array().expect("array documenti").len(), 1);
    assert_eq!(documents[0]["title"], "Referto sintetico");
    assert!(
        search
            .as_array()
            .expect("array ricerca")
            .iter()
            .any(|item| {
                item["type"] == "procedure"
                    && item["text"] == "Procedura sintetica 2026-01-04 completed"
            })
    );
    assert!(
        search
            .as_array()
            .expect("array ricerca")
            .iter()
            .any(|item| {
                item["type"] == "observation" && item["text"] == "peso 70 kg 2026-01-05"
            })
    );
    assert!(
        search
            .as_array()
            .expect("array ricerca")
            .iter()
            .any(|item| {
                item["type"] == "dicom_study"
                    && item["title"] == "TAC torace"
                    && item["text"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("viewer html non rilevato")
            })
    );

    let study = &payload["clinical"]["dicom_studies"][0];
    assert_eq!(study["instance_count"], 42);
    assert!(study["viewer_href"].is_null());
    assert_eq!(study["href"], "../documents/20260102 TAC.zip");
    assert!(
        study["fields"]
            .as_array()
            .expect("campi studio")
            .iter()
            .any(|field| {
                field["label"] == "Anomalia" && field["value"] == "anteprima non disponibile"
            })
    );
}

#[test]
fn markdown_exports_render_commonmark_and_escape_raw_html() {
    let directory = TestDirectory::new();
    let person = person(directory.path(), "patient-a");
    let document = document(&person, "20260103 Summary.md");
    fs::write(
        &document.path,
        "# Referto\n\n- Voce\n\n<script>alert(1)</script>",
    )
    .expect("Markdown sintetico");
    let metadata = CuratedMetadata {
        clinical_summary: Some(
            "# Sintesi clinica\n\n- Ipertensione.\n\n<script>alert(1)</script>\n".to_owned(),
        ),
        ..CuratedMetadata::default()
    };

    let result = generate_exports(&person, &[document], &metadata, &[], &[])
        .expect("export Markdown sintetico");
    let documents: Value =
        serde_json::from_slice(&fs::read(result.documents).expect("documenti JSON"))
            .expect("documenti validi");
    let summary: Value = serde_json::from_slice(&fs::read(result.summary).expect("sommario JSON"))
        .expect("sommario valido");

    assert_eq!(
        documents[0]["markdown_html"],
        "<h1>Referto</h1>\n<ul>\n<li>Voce</li>\n</ul>\n<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>\n"
    );
    assert_eq!(
        summary["clinical_summary_html"],
        "<h1>Sintesi clinica</h1>\n<ul>\n<li>Ipertensione.</li>\n</ul>\n<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>\n"
    );
}

#[test]
fn markdown_exports_remove_unsafe_link_and_image_destinations() {
    assert_eq!(
        render_markdown("[unsafe](javascript:alert(1))"),
        "<p>[unsafe](javascript:alert(1))</p>\n"
    );
    assert_eq!(
        render_markdown("![unsafe](data:image/png;base64,AAAA)"),
        "<p><img src=\"data:image/png;base64,AAAA\" alt=\"unsafe\" /></p>\n"
    );
    assert_eq!(
        render_markdown("![unsafe](data:text/html;base64,AAAA)"),
        "<p>![unsafe](data:text/html;base64,AAAA)</p>\n"
    );
    assert_eq!(
        render_markdown("[safe](https://example.test)"),
        "<p><a href=\"https://example.test\">safe</a></p>\n"
    );
}
