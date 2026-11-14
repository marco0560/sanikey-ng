use super::{
    BuildCounts, BuildMode, BuildOptions, PatientBuildResult, build_all, build_error,
    build_patient, collect_files, trace_patient_result_paths, trace_person_paths,
};
use crate::config::{
    AccountsConfig, IngestionConfig, PersonConfig, SearchConfig, UiConfig, UsbConfig,
};
use crate::logging::{Component, Level, Session};
use serde_json::Value;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sanikey-build-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(root: &Path, id: &str) -> PersonConfig {
    let base = root.join("patients").join(id);
    PersonConfig {
        id: id.to_owned(),
        display_name: format!("Persona {id}"),
        source_documents: base.join("documents"),
        metadata_directory: base.join("metadata"),
        local_build: root.join("generated").join(id),
        usb_uuid: "SYNTHETIC-UUID".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn build_options(mode: BuildMode) -> BuildOptions {
    BuildOptions {
        mode,
        force_pdf_ocr: false,
        parameter_debug: false,
    }
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, contents) in entries {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("membro ZIP sintetico");
        archive
            .write_all(contents)
            .expect("contenuto ZIP sintetico");
    }
    fs::write(
        path,
        archive
            .finish()
            .expect("ZIP sintetico completo")
            .into_inner(),
    )
    .expect("archivio sintetico scritto");
}

#[test]
fn incremental_build_reuses_extraction_and_invalidates_changed_sources() {
    let temp = TestDirectory::new();
    let person = person(&temp.0, "synthetic-a");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let source = person.source_documents.join("2026-01-02 report.txt");
    fs::write(&source, "Peso 70 kg\nDocumento sintetico").expect("documento sintetico");

    let first = build_patient(&person, build_options(BuildMode::Incremental))
        .expect("prima build incrementale");
    let first_checksums = fs::read_to_string(&first.checksums).expect("checksum iniziali");
    assert_eq!(first.counts.extracted_documents, 1);
    assert_eq!(first.counts.cached_documents, 0);
    assert_eq!(
        fs::read(&source).expect("originale preservato"),
        b"Peso 70 kg\nDocumento sintetico"
    );

    let second = build_patient(&person, build_options(BuildMode::Incremental))
        .expect("build incrementale ripetuta");
    assert_eq!(second.counts.extracted_documents, 0);
    assert_eq!(second.counts.cached_documents, 1);
    assert_eq!(
        fs::read_to_string(&second.checksums).expect("checksum ripetuti"),
        first_checksums
    );

    fs::write(&source, "Peso 71 kg\nDocumento sintetico aggiornato")
        .expect("modifica sintetica del documento");
    let changed = build_patient(&person, build_options(BuildMode::Incremental))
        .expect("build dopo modifica sorgente");
    assert_eq!(changed.counts.extracted_documents, 1);
    assert_eq!(changed.counts.cached_documents, 0);

    let full = build_patient(&person, build_options(BuildMode::Full)).expect("build completa");
    assert_eq!(full.counts.extracted_documents, 1);
    assert_eq!(full.counts.cached_documents, 0);
    assert_eq!(
        read_json(&full.manifest)["build_mode"],
        Value::String("full".to_owned())
    );
    let full_database = fs::read(&full.database).expect("database full");
    let incremental = build_patient(&person, build_options(BuildMode::Incremental))
        .expect("build incrementale equivalente");
    assert_eq!(incremental.counts.cached_documents, 1);
    assert_eq!(
        fs::read(&incremental.database).expect("database incrementale"),
        full_database
    );
    let validation = build_patient(&person, build_options(BuildMode::Validation))
        .expect("build validation senza cache");
    assert_eq!(validation.counts.extracted_documents, 1);
    assert_eq!(validation.counts.cached_documents, 0);
}

#[test]
fn build_all_runs_enabled_people_in_configuration_order() {
    let temp = TestDirectory::new();
    let mut first = person(&temp.0, "synthetic-a");
    first.source_documents = temp.0.join("patients/synthetic-a/documents");
    fs::create_dir_all(&first.source_documents).expect("directory prima persona");
    fs::write(
        first.source_documents.join("a.txt"),
        "Prima persona sintetica",
    )
    .expect("documento prima persona");
    let mut disabled = person(&temp.0, "disabled");
    disabled.enabled = false;
    let last = person(&temp.0, "synthetic-b");
    fs::create_dir_all(&last.source_documents).expect("directory seconda persona");
    fs::write(
        last.source_documents.join("b.txt"),
        "Seconda persona sintetica",
    )
    .expect("documento seconda persona");
    let config = AccountsConfig {
        config_version: 1,
        people: vec![first, disabled, last],
        path: temp.0.join("config/accounts.toml"),
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
        usb: UsbConfig::default(),
    };

    let results =
        build_all(&config, build_options(BuildMode::Full)).expect("build tutte le persone");

    assert_eq!(
        results
            .iter()
            .map(|result| result.patient_id.as_str())
            .collect::<Vec<_>>(),
        vec!["synthetic-a", "synthetic-b"]
    );
    assert!(results.iter().all(|result| result.database.is_file()));
}

#[test]
fn build_all_keeps_completed_output_and_resumes_after_a_later_failure() {
    let temp = TestDirectory::new();
    let first = person(&temp.0, "synthetic-a");
    fs::create_dir_all(&first.source_documents).expect("directory prima persona");
    fs::write(
        first.source_documents.join("a.txt"),
        "Prima persona sintetica",
    )
    .expect("documento prima persona");

    let second = person(&temp.0, "synthetic-b");
    fs::create_dir_all(&second.source_documents).expect("directory seconda persona");
    fs::write(
        second.source_documents.join("b.txt"),
        "Seconda persona sintetica",
    )
    .expect("documento seconda persona");
    fs::create_dir_all(
        second
            .local_build
            .parent()
            .expect("parent build seconda persona"),
    )
    .expect("directory generated");
    fs::write(&second.local_build, "blocco sintetico").expect("blocco build sintetico");

    let config = accounts_config(&temp.0, vec![first.clone(), second.clone()]);
    assert!(build_all(&config, build_options(BuildMode::Incremental)).is_err());
    assert!(
        first
            .local_build
            .join("database/medical_archive.db")
            .is_file()
    );

    fs::remove_file(&second.local_build).expect("rimozione blocco sintetico");
    let resumed = build_all(&config, build_options(BuildMode::Incremental))
        .expect("ripresa build multi-persona");
    assert_eq!(resumed.len(), 2);
    assert_eq!(resumed[0].counts.cached_documents, 1);
    assert_eq!(resumed[0].counts.extracted_documents, 0);
    assert_eq!(resumed[1].counts.extracted_documents, 1);
}

#[test]
fn invalid_cache_and_forced_ocr_mode_reextract_instead_of_reusing_entries() {
    let temp = TestDirectory::new();
    let person = person(&temp.0, "synthetic-cache");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(
        person.source_documents.join("report.txt"),
        "Testo sintetico",
    )
    .expect("documento sintetico");

    build_patient(&person, build_options(BuildMode::Incremental)).expect("prima build");
    fs::write(
        person.local_build.join("cache/extracted_text.json"),
        "{cache non valido",
    )
    .expect("cache corrotta sintetica");
    let invalidated =
        build_patient(&person, build_options(BuildMode::Incremental)).expect("cache invalida");
    assert_eq!(invalidated.counts.extracted_documents, 1);
    assert_eq!(invalidated.counts.cached_documents, 0);

    let forced = build_patient(
        &person,
        BuildOptions {
            force_pdf_ocr: true,
            ..build_options(BuildMode::Incremental)
        },
    )
    .expect("build con OCR forzato");
    assert_eq!(forced.counts.extracted_documents, 1);
    assert_eq!(forced.counts.cached_documents, 0);
}

#[test]
fn duplicate_source_content_is_built_once_and_reported() {
    let temp = TestDirectory::new();
    let person = person(&temp.0, "synthetic-duplicates");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(
        person.source_documents.join("first.txt"),
        "Contenuto sintetico",
    )
    .expect("primo documento");
    fs::write(
        person.source_documents.join("second.txt"),
        "Contenuto sintetico",
    )
    .expect("secondo documento duplicato");

    let result = build_patient(&person, build_options(BuildMode::Full)).expect("build duplicati");

    assert_eq!(result.counts.documents, 1);
    assert_eq!(result.counts.total_records, 1);
    assert_eq!(result.duplicates, 1);
    assert!(
        result
            .warning_messages
            .iter()
            .any(|warning| warning.contains("duplicato"))
    );
}

#[test]
fn archive_duplicates_are_deduplicated_but_excluded_readmes_are_ignored() {
    let temp = TestDirectory::new();
    let mut person = person(&temp.0, "synthetic-archive-duplicates");
    person.ingestion.exclude_patterns = vec!["README.md".to_owned()];
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let report = b"Peso 70 kg\nDocumento clinico sintetico";
    fs::write(person.source_documents.join("report.txt"), report).expect("documento sintetico");
    write_zip(
        &person.source_documents.join("bundle.zip"),
        &[("report.txt", report), ("README.md", report)],
    );

    let result = build_patient(&person, build_options(BuildMode::Full))
        .expect("build deduplicata con README escluso");

    assert_eq!(result.counts.documents, 2);
    assert_eq!(result.counts.derived_documents, 1);
    assert_eq!(result.counts.total_records, 2);
    assert_eq!(result.duplicates, 1);
    let duplicate_warning = result
        .warning_messages
        .iter()
        .find(|warning| warning.contains("duplicato"))
        .expect("duplicato del documento incluso segnalato");
    assert!(duplicate_warning.contains("report.txt"));
    assert!(!duplicate_warning.contains("README.md"));

    let database = rusqlite::Connection::open(&result.database).expect("database creato");
    let document_count: i64 = database
        .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .expect("conteggio documenti");
    let readme_count: i64 = database
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE path LIKE '%README.md%' OR internal_path = 'README.md'",
            [],
            |row| row.get(0),
        )
        .expect("conteggio README");
    assert_eq!(document_count, 2);
    assert_eq!(readme_count, 0);
}

#[test]
fn invalid_parameter_rules_fail_before_creating_build_output() {
    let temp = TestDirectory::new();
    let person = person(&temp.0, "synthetic-invalid-rules");
    fs::create_dir_all(&person.metadata_directory).expect("directory metadati");
    fs::write(
        person.metadata_directory.join("parameters.toml"),
        "[rules\n",
    )
    .expect("regole TOML invalide sintetiche");

    assert!(build_patient(&person, BuildOptions::default()).is_err());
    assert!(!person.local_build.exists());
}

#[test]
fn public_build_traces_record_synthetic_inputs_and_results_only_at_trace_level() {
    for (level, expect_trace) in [(Level::Events, false), (Level::Trace, true)] {
        let temp = TestDirectory::new();
        let patient = person(&temp.0, "private-synthetic-patient");
        fs::create_dir_all(&patient.metadata_directory).expect("metadata sintetici");
        fs::write(
            patient.metadata_directory.join("parameters.toml"),
            "[rules\n",
        )
        .expect("regole sintetiche invalide");
        let config = accounts_config(&temp.0, vec![patient.clone()]);
        let (session, log_path) =
            Session::create_in_directory_with_level(&temp.0.join("logs"), Component::Core, level)
                .expect("sessione sintetica");

        let (patient_result, all_result) = session.scoped(|| {
            (
                build_patient(&patient, BuildOptions::default()),
                build_all(&config, BuildOptions::default()),
            )
        });
        assert!(patient_result.is_err());
        assert!(all_result.is_err());

        let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
        if expect_trace {
            assert!(log.contains("build::build_patient"));
            assert!(log.contains("build::build_all"));
            assert!(log.contains("build::load_person_parameter_rules"));
            assert!(log.contains("private-synthetic-patient"));
            assert!(log.contains("Configuration"));
            assert!(log.contains("rust_debug"));
        } else {
            assert!(!log.contains("build::build_patient"));
            assert!(!log.contains("build::build_all"));
            assert!(!log.contains("private-synthetic-patient"));
        }
    }
}

#[test]
fn collect_files_trace_distinguishes_before_and_after_output_state() {
    let temp = TestDirectory::new();
    let scan_root = temp.0.join("scan-root");
    fs::create_dir_all(scan_root.join("nested")).expect("directory sintetica annidata");
    fs::write(scan_root.join("first-after.txt"), b"synthetic one").expect("primo file sintetico");
    fs::write(scan_root.join("nested/second-after.txt"), b"synthetic two")
        .expect("secondo file sintetico");
    let (session, log_path) = Session::create_in_directory_with_level(
        &temp.0.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");
    let mut files = vec![PathBuf::from("before-marker.txt")];

    let result = session.scoped(|| collect_files(&scan_root, &mut files, 0));

    assert!(result.is_ok());
    assert_eq!(files.len(), 3);
    let records: Vec<Value> = fs::read_to_string(log_path)
        .expect("log sintetico leggibile")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect();
    let entry = records
        .iter()
        .find(|record| {
            record["type"] == "function_enter" && record["function"] == "build::collect_files"
        })
        .expect("record di ingresso collect_files");
    let before = entry["arguments"]
        .as_array()
        .expect("argomenti JSON")
        .iter()
        .find(|argument| argument["name"] == "files_before")
        .expect("snapshot iniziale")
        .pointer("/content/value/rust_debug")
        .and_then(Value::as_str)
        .expect("vettore iniziale rappresentato");
    let exit = records
        .iter()
        .find(|record| {
            record["type"] == "function_exit"
                && record["call_id"] == entry["call_id"]
                && record["function"] == "build::collect_files"
        })
        .expect("record di uscita collect_files");
    let after = &exit["result"]["value"]["value"]["files_after"]["value"];

    assert!(before.contains("before-marker.txt"));
    assert!(!before.contains("first-after.txt"));
    assert_eq!(after["paths"].as_array().expect("path finali").len(), 3);
    assert_eq!(
        after["rust_debug"]
            .as_str()
            .unwrap()
            .matches("after.txt")
            .count(),
        2
    );
    let delta_records: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_exit"
                && record["function"] == "build::collect_files"
                && record["result"]["value"]["value"]["paths_added"].is_array()
        })
        .collect();
    let added: Vec<_> = delta_records
        .iter()
        .flat_map(|record| {
            record["result"]["value"]["value"]["paths_added"]
                .as_array()
                .unwrap()
                .iter()
        })
        .collect();
    assert_eq!(added.len(), 2);
    let mut added_indices = added
        .iter()
        .map(|item| item["index"].as_u64().unwrap())
        .collect::<Vec<_>>();
    added_indices.sort_unstable();
    assert_eq!(added_indices, vec![1, 2]);
    let mut reconstructed = entry["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|argument| argument["name"] == "files_before")
        .unwrap()["content"]["value"]["paths"]
        .as_array()
        .unwrap()
        .clone();
    let mut additions = added.into_iter().cloned().collect::<Vec<_>>();
    additions.sort_by_key(|item| item["index"].as_u64().unwrap());
    for addition in additions {
        assert_eq!(
            addition["index"].as_u64().unwrap() as usize,
            reconstructed.len()
        );
        reconstructed.push(serde_json::json!({
            "index": addition["index"],
            "path": addition["path"],
        }));
    }
    assert_eq!(&reconstructed, after["paths"].as_array().unwrap());
    let root_call_id = entry["call_id"].clone();
    let call_entries = records
        .iter()
        .filter(|record| {
            record["type"] == "function_enter" && record["function"] == "build::collect_files"
        })
        .collect::<Vec<_>>();
    let call_ids = call_entries
        .iter()
        .map(|record| record["call_id"].clone())
        .collect::<Vec<_>>();
    assert!(call_ids.contains(&root_call_id));
    assert!(
        call_entries.len() >= 2,
        "fixture annidata tracciata per frame"
    );

    for call in call_entries {
        let arguments = call["arguments"].as_array().unwrap();
        let depth = arguments
            .iter()
            .find(|argument| argument["name"] == "depth")
            .unwrap()["content"]["value"]["value"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let length_at_entry = if depth == 0 {
            arguments
                .iter()
                .find(|argument| argument["name"] == "files_before")
                .unwrap()["content"]["value"]["paths"]
                .as_array()
                .unwrap()
                .len()
        } else {
            arguments
                .iter()
                .find(|argument| argument["name"] == "files_state_reference")
                .unwrap()["content"]["value"]["length_at_entry"]
                .as_u64()
                .unwrap() as usize
        };
        let exit = records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "build::collect_files"
                    && record["call_id"] == call["call_id"]
            })
            .expect("uscita del frame correlata con call_id");
        let length_after = exit["result"]["value"]["value"]["files_length_after"]
            .as_u64()
            .unwrap() as usize;
        assert!(length_at_entry <= length_after);
        assert!(length_after <= after["paths"].as_array().unwrap().len());
        assert_eq!(
            &after["paths"].as_array().unwrap()[..length_at_entry],
            &reconstructed[..length_at_entry],
            "input ricostruito per call_id {}",
            call["call_id"]
        );
        assert_eq!(
            &after["paths"].as_array().unwrap()[..length_after],
            &reconstructed[..length_after],
            "output ricostruito per call_id {}",
            call["call_id"]
        );
    }
}

#[cfg(unix)]
#[test]
fn trace_path_projections_preserve_non_unicode_nested_and_result_paths() {
    use std::os::unix::ffi::OsStringExt;

    let root = PathBuf::from(std::ffi::OsString::from_vec(
        b"synthetic-\xff-root".to_vec(),
    ));
    let mut person = person(&root, "synthetic-person");
    person.ui.background_image = Some(PathBuf::from(std::ffi::OsString::from_vec(
        b"synthetic-\xfe-background.png".to_vec(),
    )));
    person.search.dictionary = Some(PathBuf::from(std::ffi::OsString::from_vec(
        b"synthetic-\xfd-dictionary.toml".to_vec(),
    )));
    let person_paths = trace_person_paths(&person).to_json();
    let paths = person_paths["value"]
        .as_array()
        .expect("proiezione dei path");
    assert_eq!(paths[0]["path"]["value"]["encoding"], "unix_bytes");
    assert_eq!(paths[0]["path"]["value"]["value"][0], b's');
    assert_eq!(paths[3]["path"]["value"]["value"][10], 0xfe);
    assert_eq!(paths[4]["path"]["value"]["value"][10], 0xfd);

    let result = Ok(PatientBuildResult {
        patient_id: person.id,
        build_root: root.join("build"),
        counts: BuildCounts::default(),
        duplicates: 0,
        warnings: 0,
        warning_messages: Vec::new(),
        database: root.join("db.sqlite"),
        manifest: root.join("manifest.json"),
        checksums: root.join("checksums.sha256"),
        report: root.join("report.json"),
    });
    let result_paths = trace_patient_result_paths(&result).to_json();
    assert_eq!(result_paths["value"]["state"], "complete");
    assert_eq!(
        result_paths["value"]["paths"][0]["path"]["value"]["encoding"],
        "unix_bytes"
    );

    let temp = TestDirectory::new();
    let directory = temp.0.join("scan");
    fs::create_dir_all(&directory).expect("directory scan sintetica");
    let non_unicode_name = std::ffi::OsString::from_vec(b"scan-\xfc.txt".to_vec());
    fs::write(directory.join(non_unicode_name), b"synthetic").expect("file sintetico");
    let (session, log_path) = Session::create_in_directory_with_level(
        &temp.0.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");
    let mut files = Vec::new();
    assert!(
        session
            .scoped(|| collect_files(&directory, &mut files, 0))
            .is_ok()
    );
    let records: Vec<Value> = fs::read_to_string(log_path)
        .expect("log sintetico leggibile")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect();
    let root_exit = records
        .iter()
        .find(|record| {
            record["type"] == "function_exit"
                && record["function"] == "build::collect_files"
                && record["result"]["value"]["value"]["depth"] == 0
        })
        .expect("risultato root collect_files");
    let path_bytes =
        root_exit["result"]["value"]["value"]["files_after"]["value"]["paths"][0]["path"]["value"]
            ["value"]
            .as_array()
            .expect("byte UTF-8 nativi");
    assert!(path_bytes.iter().any(|byte| byte == 0xfc));
}

#[test]
fn collect_files_trace_keeps_prepopulated_state_on_root_error() {
    let temp = TestDirectory::new();
    let missing = temp.0.join("missing-directory");
    let (session, log_path) = Session::create_in_directory_with_level(
        &temp.0.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");
    let marker = temp.0.join("prepopulated-marker");
    let mut files = vec![marker.clone()];

    let result = session.scoped(|| collect_files(&missing, &mut files, 0));

    assert!(result.is_err());
    assert_eq!(files, vec![marker]);
    let records: Vec<Value> = fs::read_to_string(log_path)
        .expect("log sintetico leggibile")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect();
    let exit = records
        .iter()
        .find(|record| {
            record["type"] == "function_exit" && record["function"] == "build::collect_files"
        })
        .expect("risultato errore collect_files");
    assert!(
        exit["result"]["value"]["value"]["return"]
            .as_str()
            .unwrap()
            .contains("Err")
    );
    assert_eq!(exit["result"]["value"]["value"]["files_length_after"], 1);
    assert_eq!(
        exit["result"]["value"]["value"]["files_after"]["value"]["paths"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn collect_files_delta_replay_uses_absolute_index_not_exit_sequence() {
    let initial = vec![serde_json::json!({"index":0,"path":"prepopulated"})];
    // Simula un figlio che termina prima del padre: il padre aveva aggiunto
    // il proprio file (indice 1) prima della discesa, ma registra l'uscita dopo.
    let deltas = [
        serde_json::json!({"sequence":12,"index":1,"path":"parent-file"}),
        serde_json::json!({"sequence":11,"index":2,"path":"child-file"}),
    ];
    assert!(deltas[0]["sequence"].as_u64().unwrap() > deltas[1]["sequence"].as_u64().unwrap());
    let mut replay = initial;
    let mut ordered = deltas.to_vec();
    ordered.sort_by_key(|delta| delta["index"].as_u64().unwrap());
    for delta in ordered {
        assert_eq!(delta["index"].as_u64().unwrap() as usize, replay.len());
        replay.push(serde_json::json!({
            "index": delta["index"],
            "path": delta["path"],
        }));
    }
    assert_eq!(
        replay
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["prepopulated", "parent-file", "child-file"]
    );
}

#[test]
fn collect_files_does_not_write_trace_records_at_events_level() {
    let temp = TestDirectory::new();
    let directory = temp.0.join("scan");
    fs::create_dir_all(&directory).expect("directory scan sintetica");
    fs::write(directory.join("file.txt"), b"synthetic").expect("file sintetico");
    let (session, log_path) = Session::create_in_directory_with_level(
        &temp.0.join("logs"),
        Component::Core,
        Level::Events,
    )
    .expect("sessione events sintetica");
    let mut files = Vec::new();
    assert!(
        session
            .scoped(|| collect_files(&directory, &mut files, 0))
            .is_ok()
    );
    let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
    assert!(!log.contains("build::collect_files"));
    assert!(!log.contains("paths_added"));
}

#[test]
fn build_error_trace_redacts_secret_like_debug_text_without_changing_error() {
    let temp = TestDirectory::new();
    let (session, log_path) = Session::create_in_directory_with_level(
        &temp.0.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");
    let message = "password = synthetic-build-secret";
    let error = session.scoped(|| build_error(message));
    assert_eq!(error.message(), message);
    drop(session);

    let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
    assert!(log.contains("secret_omitted"));
    assert!(!log.contains("synthetic-build-secret"));
}

fn accounts_config(root: &Path, people: Vec<PersonConfig>) -> AccountsConfig {
    AccountsConfig {
        config_version: 1,
        people,
        path: root.join("config/accounts.toml"),
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
        usb: UsbConfig::default(),
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("JSON generato leggibile"))
        .expect("JSON generato valido")
}
