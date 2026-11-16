//! Verifica sintetica delle span Trace della copia di media DICOM.

use sanikey_core::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use sanikey_core::dicom::{DicomStudy, prepare_dicom_media};
use sanikey_core::logging::{Component, Level, Session};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Crea e fornisce accesso alla directory sintetica del test.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-dicom-media-{}-{}",
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

/// Rimuove la directory sintetica al termine del fixture.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn path_value(path: &Path) -> Value {
    json!({
        "kind": "data",
        "value": {"encoding": "utf8", "value": path.to_str().expect("path UTF-8")}
    })
}

fn field(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn arguments(records: &[Value], function: &str) -> (u64, Value) {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), 1, "una sola entrata per {function}");
    (
        entries[0]["call_id"].as_u64().expect("call_id numerico"),
        entries[0]["arguments"].clone(),
    )
}

fn result(records: &[Value], function: &str, call_id: u64, expected: Value) {
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
    assert_eq!(exits[0]["result"], expected, "risultato {function}");
}

fn trace_records(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("log Trace sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect()
}

#[test]
fn dicom_media_copy_traces_paths_result_call_ids_duration_and_binary_omission() {
    let directory = TestDirectory::new();
    let local_build = directory.path().join("build");
    let source_root = local_build.join("staging/containers/study-a");
    fs::create_dir_all(&source_root).expect("staging DICOM sintetico");
    let source = source_root.join("image.dcm");
    let mut binary = vec![0_u8; 132];
    binary[..4].copy_from_slice(&[0, 255, 17, 128]);
    binary[128..].copy_from_slice(b"DICM");
    fs::write(&source, &binary).expect("istanza DICOM sintetica");
    let person = PersonConfig {
        id: "synthetic-person".to_owned(),
        display_name: "Persona sintetica".to_owned(),
        source_documents: directory.path().join("source"),
        metadata_directory: directory.path().join("metadata"),
        local_build: local_build.clone(),
        usb_uuid: "SYNTHETIC-UUID".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    };
    let study = DicomStudy {
        study_id: "study-a".to_owned(),
        patient_id: person.id.clone(),
        support_path: directory.path().join("source/study.iso"),
        support_kind: "dicom_iso".to_owned(),
        extracted_path: Some(source_root.clone()),
        viewer_paths: Vec::new(),
        html_viewer_path: None,
        warnings: Vec::new(),
        study_instance_uid: None,
        study_date: None,
        study_description: None,
        instance_count: 1,
        support_paths: vec![source.clone()],
    };
    let logs = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");

    let media = session
        .scoped(|| prepare_dicom_media(&person, std::slice::from_ref(&study)))
        .expect("preparazione media sintetico");
    assert_eq!(media.len(), 1);
    assert_eq!(media[0].instance_count, 1);
    assert_eq!(
        fs::read(media[0].directory.join("image.dcm")).unwrap(),
        binary
    );
    assert_eq!(fs::read(&source).unwrap(), binary);
    session.finish(Component::Core).expect("chiusura sessione");

    let records = trace_records(&log_path);
    let media_id = "db6ec5b85faf600b";
    let target = local_build
        .join("dicom-media")
        .join(media_id)
        .join("image.dcm");
    let media_directory = local_build.join("dicom-media").join(media_id);
    let expected_person_text = format!("{person:#?}");
    let expected_study = json!({
        "study_id": "study-a",
        "patient_id": "synthetic-person",
        "support_path": path_value(&study.support_path),
        "support_kind": "dicom_iso",
        "extracted_path": path_value(&source_root),
        "viewer_paths": {"kind":"data", "value":[]},
        "html_viewer_path": null,
        "warnings": [],
        "study_instance_uid": null,
        "study_date": null,
        "study_description": null,
        "instance_count": 1,
        "support_paths": {"kind":"data", "value":[{"index":0,"path":path_value(&source)}]},
    });

    let (prepare_id, prepare_args) = arguments(&records, "dicom::prepare_dicom_media");
    assert_eq!(
        prepare_args,
        json!([
            field(
                "person",
                json!({"kind":"data", "value":{"format":"text_redacted_known_secret_assignments", "value":expected_person_text}})
            ),
            field(
                "person_paths",
                json!({"kind":"data", "value":{
                    "source_documents":path_value(&person.source_documents),
                    "metadata_directory":path_value(&person.metadata_directory),
                    "local_build":path_value(&person.local_build),
                    "background_image":null,
                    "search_dictionary":null,
                }})
            ),
            field(
                "studies",
                json!({"kind":"data", "value":[{"kind":"data", "value":expected_study}]})
            ),
        ])
    );
    let prepare_output = json!({"kind":"data", "value":{"output":{"kind":"data", "value":[{"kind":"data", "value":{
        "media_id":media_id,
        "study_ids":["study-a"],
        "directory":path_value(&media_directory),
        "instance_count":1,
        "warnings":[format!("{}: DICOMDIR assente; istanze originali conservate senza indice standard", source_root.display())],
    }}]}}});
    result(
        &records,
        "dicom::prepare_dicom_media",
        prepare_id,
        prepare_output,
    );

    let (copy_id, copy_args) = arguments(&records, "dicom::copy_file");
    assert_eq!(
        copy_args,
        json!([
            field("source", path_value(&source)),
            field("target", path_value(&target)),
            field(
                "transferred_contents",
                json!({"kind":"omitted", "reason":"external_component"})
            ),
        ])
    );
    assert_ne!(prepare_id, copy_id);
    result(
        &records,
        "dicom::copy_file",
        copy_id,
        json!({"kind":"data", "value":{"output":{"kind":"data", "value":{
            "copied":true,
            "bytes_copied":binary.len(),
            "warnings_added":[],
        }}}}),
    );
}
