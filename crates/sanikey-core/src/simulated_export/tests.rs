//! Fixture sintetiche per export e validazione dell'immagine TXT.

use super::{
    count_files, export_txt_simulation, percent_decode, safe_relative_path, sha256_file,
    validate_txt_simulation,
};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::logging::{Component, Level, Session};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

/// Gestisce la directory temporanea per gli export sintetici.
impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-simulated-export-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Elimina gli artefatti di export creati dal test.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn person(root: &Path) -> PersonConfig {
    let corpus = repository_root().join("docs/migration/corpus/minimal/patient-a");
    PersonConfig {
        id: "patient-a".to_owned(),
        display_name: "Paziente sintetico A".to_owned(),
        source_documents: corpus.join("documents"),
        metadata_directory: corpus.join("metadata"),
        local_build: root.join("generated"),
        usb_uuid: "1A2B-3C4D".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn read_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("directory sintetica leggibile") {
            let entry = entry.expect("elemento sintetico leggibile");
            let path = entry.path();
            if entry
                .file_type()
                .expect("tipo sintetico leggibile")
                .is_dir()
            {
                pending.push(path);
            } else if entry
                .file_type()
                .expect("tipo sintetico leggibile")
                .is_file()
            {
                files.push((
                    path.strip_prefix(root)
                        .expect("percorso relativo al corpus")
                        .to_path_buf(),
                    fs::read(path).expect("byte sintetici"),
                ));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
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
        "value": {
            "kind": "data",
            "value": {
                "encoding": "utf8",
                "value": path.to_str().expect("percorso fixture UTF-8"),
            }
        }
    })
}

fn expected_secret_path_trace() -> Value {
    json!({"kind": "data", "value": {"kind": "secret_omitted"}})
}

fn expected_arguments(fields: Vec<(&str, Value)>) -> Value {
    Value::Array(
        fields
            .into_iter()
            .map(|(name, content)| json!({"name": name, "content": content}))
            .collect(),
    )
}

fn expected_debug_result<T: std::fmt::Debug>(value: &T) -> Value {
    expected_text_trace(&format!("{value:#?}"))
}

fn assert_complete_trace_calls(records: &[Value], function: &str, expected: Vec<(Value, Value)>) {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), expected.len(), "numero ingressi: {function}");
    let mut unmatched = entries;
    for (arguments, result) in expected {
        let index = unmatched
            .iter()
            .position(|entry| entry["arguments"] == arguments)
            .unwrap_or_else(|| panic!("argomenti completi assenti per {function}: {arguments}"));
        let entry = unmatched.remove(index);
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        let exits: Vec<_> = records
            .iter()
            .filter(|record| {
                record["type"] == "function_exit"
                    && record["function"] == function
                    && record["call_id"] == call_id
            })
            .collect();
        assert_eq!(exits.len(), 1, "uscita correlata: {function}#{call_id}");
        assert_eq!(exits[0]["outcome"], "returned", "esito: {function}");
        assert!(
            exits[0]["duration_ms"].is_u64(),
            "durata assente: {function}#{call_id}"
        );
        assert_eq!(exits[0]["result"], result, "risultato: {function}");
    }
    assert!(unmatched.is_empty());
}

#[test]
fn traced_export_helpers_match_complete_inputs_results_and_omissions() {
    let directory = TestDirectory::new();
    let safe_input = "patients/patient-a/documents/Report One.txt";
    let safe_result = PathBuf::from("patients/patient-a/documents/Report One.txt");
    let encoded_input = "patients/patient-a/documents/Report%20One.txt";
    let decoded_result = "patients/patient-a/documents/Report One.txt";
    let secret_path = directory
        .path()
        .join("api_key=SYNTHETIC-EXPORT-PATH-SECRET");
    fs::write(&secret_path, b"abc").expect("file sintetico");
    let tree = directory.path().join("tree");
    fs::create_dir_all(tree.join("nested")).expect("tree sintetico");
    fs::write(tree.join("one.txt"), b"one").expect("file sintetico");
    fs::write(tree.join("nested/two.bin"), b"two").expect("file sintetico");

    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");
    let (safe, decoded, digest, file_count) = session.scoped(|| {
        (
            safe_relative_path(safe_input).expect("path relativo sicuro"),
            percent_decode(encoded_input).expect("percent encoding valido"),
            sha256_file(&secret_path).expect("checksum file sintetico"),
            count_files(&tree).expect("conteggio file sintetici"),
        )
    });
    drop(session);

    assert_eq!(safe, safe_result);
    assert_eq!(decoded, decoded_result);
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(file_count, 2);
    let log = fs::read_to_string(log_path).expect("log trace sintetico");
    assert!(!log.contains("SYNTHETIC-EXPORT-PATH-SECRET"));
    assert!(log.contains("secret_omitted"));
    let records: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
        .collect();

    let safe_arguments = expected_arguments(vec![("value", expected_text_trace(safe_input))]);
    assert_complete_trace_calls(
        &records,
        "simulated_export::safe_relative_path",
        vec![(safe_arguments.clone(), expected_debug_result(&safe_result))],
    );
    assert_complete_trace_calls(
        &records,
        "simulated_export::safe_relative_path_inner",
        vec![(safe_arguments, expected_debug_result(&safe_result))],
    );

    let percent_arguments = expected_arguments(vec![("value", expected_text_trace(encoded_input))]);
    assert_complete_trace_calls(
        &records,
        "simulated_export::percent_decode",
        vec![(
            percent_arguments.clone(),
            expected_debug_result(&decoded_result),
        )],
    );
    assert_complete_trace_calls(
        &records,
        "simulated_export::percent_decode_inner",
        vec![(percent_arguments, expected_debug_result(&decoded_result))],
    );

    let omitted_raw_bytes = json!({
        "kind": "data",
        "value": [
            {"name": "state", "content": {"kind": "data", "value": "omitted"}},
            {"name": "reason", "content": {"kind": "data", "value": "raw_file_bytes_not_recorded"}},
        ]
    });
    let sha_arguments = expected_arguments(vec![
        ("path", expected_secret_path_trace()),
        ("byte_length", json!({"kind": "data", "value": 3})),
        ("raw_bytes", omitted_raw_bytes),
    ]);
    assert_complete_trace_calls(
        &records,
        "simulated_export::sha256_file",
        vec![(sha_arguments, expected_debug_result(&digest))],
    );
    assert_complete_trace_calls(
        &records,
        "simulated_export::sha256_file_inner",
        vec![(
            expected_arguments(vec![("path", expected_secret_path_trace())]),
            expected_debug_result(&digest),
        )],
    );

    let count_arguments = expected_arguments(vec![("root", expected_path_trace(&tree))]);
    assert_complete_trace_calls(
        &records,
        "simulated_export::count_files",
        vec![(count_arguments.clone(), expected_debug_result(&file_count))],
    );
    assert_complete_trace_calls(
        &records,
        "simulated_export::count_files_inner",
        vec![(count_arguments, expected_debug_result(&file_count))],
    );
}

#[test]
fn simulated_export_is_repeatable_detects_tampering_and_preserves_originals() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let originals_before = read_tree(&person.source_documents);
    let first = export_txt_simulation(&person, directory.path().join("usb-first"))
        .expect("prima esportazione simulata");
    let second = export_txt_simulation(&person, directory.path().join("usb-second"))
        .expect("seconda esportazione simulata");

    assert_eq!(first.patients, 1);
    assert_eq!(first.files, second.files);
    assert_eq!(
        fs::read(&first.manifest).expect("primo manifest"),
        fs::read(&second.manifest).expect("secondo manifest")
    );
    validate_txt_simulation(&first.root).expect("prima esportazione valida");
    validate_txt_simulation(&second.root).expect("seconda esportazione valida");

    let linked_document = first
        .root
        .join("patients/patient-a/documents/duplicate/20260102 Copia sintetica.txt");
    assert!(linked_document.is_file());
    let page = first.root.join("patients/patient-a/web/index.html");
    assert!(
        fs::read_to_string(page)
            .expect("pagina nel target simulato")
            .contains("../documents/duplicate/20260102%20Copia%20sintetica.txt")
    );

    fs::write(&linked_document, b"tampered synthetic copy").expect("mutazione sintetica");
    assert!(validate_txt_simulation(&first.root).is_err());
    assert_eq!(read_tree(&person.source_documents), originals_before);
}

#[test]
fn simulated_export_rejects_existing_targets_and_input_overlaps() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let existing = directory.path().join("existing");
    fs::create_dir(&existing).expect("target preesistente");
    assert!(export_txt_simulation(&person, &existing).is_err());
    assert!(export_txt_simulation(&person, &person.local_build).is_err());
}

#[test]
fn trace_records_private_export_data_and_redacts_identified_assignments() {
    let directory = TestDirectory::new();
    let mut person = person(directory.path());
    let private_label = "Persona sintetica riservata 5291";
    let secret = "synthetic-secret-export-8043";
    person.display_name = format!("{private_label} password={secret}");
    let target = directory.path().join("usb-trace");
    let log_directory = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");

    let exported = session.scoped(|| {
        let exported = export_txt_simulation(&person, &target).expect("export sintetico");
        validate_txt_simulation(&exported.root).expect("validazione sintetica");
        exported
    });
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&exported.manifest).expect("manifest sintetico"))
            .expect("manifest JSON sintetico");
    assert_eq!(manifest["patients"][0]["display_name"], person.display_name);
    let root_index =
        fs::read_to_string(exported.root.join("index.html")).expect("indice export sintetico");
    assert!(root_index.contains(&person.display_name));
    drop(session);

    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(log.contains(private_label));
    assert!(log.contains("secret_omitted"));
    assert!(
        !log.contains(secret),
        "secret escaped through trace record(s): {}",
        log.lines()
            .filter(|line| line.contains(secret))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let records = log
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("record JSON valido"))
        .collect::<Vec<_>>();
    let inner_functions = [
        "simulated_export::export_txt_simulation_inner",
        "simulated_export::validate_txt_simulation_inner",
        "simulated_export::validate_patient_inner",
        "simulated_export::validate_document_links_inner",
        "simulated_export::validate_checksums_inner",
        "simulated_export::write_manifest_inner",
        "simulated_export::write_root_index_inner",
        "simulated_export::copy_tree_inner",
        "simulated_export::copy_file_inner",
        "simulated_export::checksum_tree_inner",
        "simulated_export::collect_relative_files_inner",
        "simulated_export::safe_relative_path_inner",
        "simulated_export::percent_decode_inner",
        "simulated_export::sha256_file_inner",
        "simulated_export::count_files_inner",
        "simulated_export::absolute_path_inner",
        "simulated_export::reject_input_overlap_inner",
    ];
    for function in inner_functions {
        let call_ids = records
            .iter()
            .filter(|record| record["type"] == "function_enter" && record["function"] == function)
            .map(|record| record["call_id"].as_u64().expect("call ID"))
            .collect::<Vec<_>>();
        assert!(!call_ids.is_empty(), "span inner assente: {function}");
        for call_id in call_ids {
            assert!(
                records.iter().any(|record| {
                    record["type"] == "function_exit"
                        && record["call_id"] == call_id
                        && record["duration_ms"].as_u64().is_some()
                }),
                "span inner senza exit/durata: {function} #{call_id}"
            );
        }
    }
    let outer_call_id = records
        .iter()
        .find(|record| {
            record["type"] == "function_enter"
                && record["function"] == "simulated_export::export_txt_simulation"
        })
        .expect("span wrapper export")["call_id"]
        .as_u64()
        .expect("call ID wrapper");
    assert!(records.iter().any(|record| {
        record["type"] == "function_enter"
            && record["function"] == "simulated_export::export_txt_simulation_inner"
            && record["parent_call_id"] == outer_call_id
    }));
}
