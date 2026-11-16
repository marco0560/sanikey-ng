//! Fixture sintetiche per estrazione e staging dei contenitori.

use super::{
    extract_process_to_file, is_container_path, parse_7z_listing, safe_member_path,
    stage_container_documents, write_member,
};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::inventory::scan_document_inventory;
use crate::logging::{Component, Level, Session};
use crate::metadata::DocumentRecord;
use serde_json::{Value, json};
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Gestisce le directory temporanee create dai test contenitori.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-containers-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Rimuove gli artefatti temporanei al termine del test.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(root: &Path) -> PersonConfig {
    PersonConfig {
        id: "patient-a".to_owned(),
        display_name: "Patient A".to_owned(),
        source_documents: root.join("documents"),
        metadata_directory: root.join("metadata"),
        local_build: root.join("generated"),
        usb_uuid: "1A2B-3C4D".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    zip_bytes_with_method(entries, zip::CompressionMethod::Deflated)
}

fn zip_bytes_with_method(entries: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default().compression_method(method),
            )
            .expect("membro ZIP sintetico");
        writer.write_all(bytes).expect("contenuto ZIP sintetico");
    }
    writer.finish().expect("ZIP sintetico").into_inner()
}

#[test]
fn reads_python_zipfile_compression_methods() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    for (name, method) in [
        ("deflate.zip", zip::CompressionMethod::Deflated),
        ("bzip2.zip", zip::CompressionMethod::Bzip2),
    ] {
        fs::write(
            person.source_documents.join(name),
            zip_bytes_with_method(&[("report.txt", b"Synthetic report")], method),
        )
        .expect("archivio ZIP sintetico");
    }
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/zip-lzma.zip"),
        person.source_documents.join("lzma.zip"),
    )
    .expect("fixture ZIP LZMA sintetica");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging ZIP con i metodi della baseline");

    assert_eq!(result.documents.len(), 3);
    assert!(result.warning_messages.is_empty());
    for document in result.documents {
        assert_eq!(
            fs::read(document.path).expect("contenuto ZIP"),
            b"Synthetic report"
        );
    }
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    fs::write(path, zip_bytes(entries)).expect("archivio ZIP sintetico");
}

fn source_inventory(person: &PersonConfig) -> Vec<crate::metadata::DocumentRecord> {
    scan_document_inventory(person).expect("inventario sintetico")
}

fn command_available(name: &str) -> bool {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).any(|directory| {
        [name.to_owned(), format!("{name}.exe")]
            .iter()
            .any(|candidate| directory.join(candidate).is_file())
    })
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
        "value": {"encoding": "utf8", "value": path.to_str().expect("path sintetico UTF-8")}
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
            .unwrap_or_else(|| {
                panic!(
                    "argomenti completi assenti per {function}: {arguments}; osservati: {unmatched:?}"
                )
            });
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
fn traced_container_helpers_match_complete_arguments_results_and_omissions() {
    let directory = TestDirectory::new();
    let container_path = Path::new("archive.TAR.XZ");
    let target = directory.path().join("staging");
    let member_name = "./reports/Report.txt";
    let expected_member_path = target.join("reports/Report.txt");
    let listing = concat!(
        "7-Zip synthetic listing\n",
        "----------\n",
        "Path = reports\n",
        "Folder = +\n",
        "----------\n",
        "Path = api_key=SYNTHETIC-CONTAINER-NAME/report.txt\n",
        "Size = 3\n",
        "----------\n",
        "Path = ignored.bin\n",
        "Folder = +\n",
    );
    let secret_member_path = "api_key=[secret_omitted]";
    let output_path = directory.path().join("members/nested/report.txt");
    let member_bytes = b"SYNTHETIC-CONTAINER-CONTENT";
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");

    let (is_container, parsed, safe_path, write_result) = session.scoped(|| {
        let is_container = is_container_path(container_path);
        let parsed = parse_7z_listing(listing);
        let safe_path = safe_member_path(&target, member_name);
        let mut input = Cursor::new(member_bytes.as_slice());
        let write_result = write_member(&mut input, &output_path);
        (is_container, parsed, safe_path, write_result)
    });
    drop(session);

    assert!(is_container);
    assert_eq!(
        parsed,
        ["api_key=SYNTHETIC-CONTAINER-NAME/report.txt".to_owned()]
    );
    assert_eq!(safe_path, Ok(expected_member_path.clone()));
    write_result.expect("scrittura membro sintetico");
    assert_eq!(
        fs::read(&output_path).expect("membro scritto"),
        member_bytes
    );

    let log = fs::read_to_string(log_path).expect("log trace sintetico");
    assert!(!log.contains("SYNTHETIC-CONTAINER-NAME"));
    assert!(!log.contains("SYNTHETIC-CONTAINER-CONTENT"));
    assert!(log.contains("secret_omitted"));
    assert!(log.contains("external_component"));
    assert!(log.contains("unsupported_type"));
    let records: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
        .collect();

    let container_arguments =
        expected_arguments(vec![("path", expected_path_trace(container_path))]);
    assert_complete_trace_calls(
        &records,
        "containers::is_container_path",
        vec![(
            container_arguments.clone(),
            json!({"kind": "data", "value": true}),
        )],
    );
    assert_complete_trace_calls(
        &records,
        "containers::is_container_path_impl",
        vec![(container_arguments, json!({"kind": "data", "value": true}))],
    );

    let redacted_listing =
        listing.replace("SYNTHETIC-CONTAINER-NAME/report.txt", "[secret_omitted]");
    let listing_arguments = expected_arguments(vec![
        ("listing", expected_text_trace(&redacted_listing)),
        (
            "encoding",
            json!({"kind": "data", "value": "utf8_lossy_upstream"}),
        ),
    ]);
    let parsed_result = json!({
        "kind": "data",
        "value": [expected_text_trace(secret_member_path)],
    });
    assert_complete_trace_calls(
        &records,
        "containers::parse_7z_listing",
        vec![(listing_arguments, parsed_result.clone())],
    );
    assert_complete_trace_calls(
        &records,
        "containers::parse_7z_listing_impl",
        vec![(
            expected_arguments(vec![("listing", expected_text_trace(&redacted_listing))]),
            parsed_result,
        )],
    );

    let safe_arguments = expected_arguments(vec![
        ("target", expected_path_trace(&target)),
        ("member_name", expected_text_trace(member_name)),
    ]);
    let safe_result = expected_path_trace(&expected_member_path);
    assert_complete_trace_calls(
        &records,
        "containers::safe_member_path",
        vec![(safe_arguments.clone(), safe_result.clone())],
    );
    assert_complete_trace_calls(
        &records,
        "containers::safe_member_path_impl",
        vec![(safe_arguments, safe_result)],
    );

    let output_arguments = expected_arguments(vec![
        ("path", expected_path_trace(&output_path)),
        (
            "member_contents",
            json!({"kind": "omitted", "reason": "external_component"}),
        ),
    ]);
    assert_complete_trace_calls(
        &records,
        "containers::write_member",
        vec![(
            output_arguments,
            json!({"kind": "data", "value": "succeeded"}),
        )],
    );
    let create_result = json!({
        "kind": "data",
        "value": [
            {"name": "state", "content": {"kind": "data", "value": "succeeded"}},
            {"name": "file_handle", "content": {"kind": "omitted", "reason": "unsupported_type"}},
        ]
    });
    assert_complete_trace_calls(
        &records,
        "containers::create_member",
        vec![(
            expected_arguments(vec![("path", expected_path_trace(&output_path))]),
            create_result,
        )],
    );
}

#[cfg(target_os = "linux")]
#[test]
fn extract_process_trace_records_status_and_unobserved_streams_without_changing_file_output() {
    let directory = TestDirectory::new();
    let (trace_session, trace_path) = Session::create_in_directory_with_level(
        &directory.path().join("process-trace"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace processo sintetico");

    for (name, code) in [("success", 0), ("failure", 37)] {
        let output_path = directory.path().join(format!("{name}.member"));
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("printf 'SYNTHETIC_MEMBER'; printf 'SYNTHETIC_STDERR' >&2; exit \"$1\"")
            .arg("synthetic-extractor")
            .arg(code.to_string());
        let result = trace_session.scoped(|| {
            extract_process_to_file(
                &mut command,
                fs::File::create(&output_path).expect("file membro sintetico"),
                None,
            )
        });
        if code == 0 {
            result.expect("estrazione sintetica riuscita");
        } else {
            assert_eq!(
                result.expect_err("exit non zero atteso"),
                "estrazione del membro non riuscita"
            );
        }
        assert_eq!(
            fs::read(output_path).expect("output del processo sintetico"),
            b"SYNTHETIC_MEMBER"
        );
    }
    drop(trace_session);

    let trace_records: Vec<Value> = fs::read_to_string(trace_path)
        .expect("log Trace del processo")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record Trace JSON valido"))
        .collect();
    let entries: Vec<_> = trace_records
        .iter()
        .filter(|record| {
            record["type"] == "function_enter"
                && record["function"] == "containers::extract_process_to_file_impl"
        })
        .collect();
    assert_eq!(entries.len(), 2);
    for (entry, (expected_code, expected_state)) in
        entries.into_iter().zip([(0, "succeeded"), (37, "error")])
    {
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        let arguments = entry["arguments"].to_string();
        assert!(arguments.contains("stdout_capture"));
        assert!(arguments.contains("stderr_capture"));
        assert!(arguments.contains("member_file_contents"));
        let exit = trace_records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "containers::extract_process_to_file_impl"
                    && record["call_id"] == call_id
            })
            .expect("uscita correlata tramite call_id");
        assert!(exit["duration_ms"].as_u64().is_some());
        let output = &exit["result"]["value"];
        assert_eq!(output["process_state"], "exited");
        assert_eq!(output["exit_code"], expected_code);
        assert!(output["signal"].is_null());
        assert_eq!(output["result"]["state"], expected_state);
        for stream in ["stdout", "stderr", "member_file_contents"] {
            assert_eq!(output[stream]["kind"], "omitted");
            assert_eq!(output[stream]["reason"], "external_component");
        }
    }

    let (events_session, events_path) = Session::create_in_directory_with_level(
        &directory.path().join("process-events"),
        Component::Core,
        Level::Events,
    )
    .expect("sessione Events processo sintetico");
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("printf 'SYNTHETIC_MEMBER'; printf 'SYNTHETIC_STDERR' >&2; exit 0");
    events_session
        .scoped(|| {
            extract_process_to_file(
                &mut command,
                fs::File::create(directory.path().join("events.member"))
                    .expect("file membro Events sintetico"),
                None,
            )
        })
        .expect("estrazione Events sintetica");
    drop(events_session);
    let events_log = fs::read_to_string(events_path).expect("log Events del processo");
    assert!(!events_log.contains("SYNTHETIC_MEMBER"));
    assert!(!events_log.contains("SYNTHETIC_STDERR"));
    assert!(!events_log.contains("function_enter"));
}

#[test]
fn stages_zip_members_and_writes_a_provenance_manifest() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let archive = person.source_documents.join("archive.ZIP");
    write_zip(
        &archive,
        &[
            ("reports/20260102_result.txt", b"Synthetic report"),
            ("technical/source.bin", b"ignored by the document pipeline"),
        ],
    );

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging ZIP sintetico");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(result.members.len(), 2);
    assert_eq!(
        result.documents[0].internal_path.as_deref(),
        Some("reports/20260102_result.txt")
    );
    assert_eq!(result.documents[0].origin, "container");
    assert!(
        result.documents[0]
            .path
            .starts_with(person.local_build.join("staging/containers"))
    );
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro estratto"),
        b"Synthetic report"
    );

    let manifest: Value = serde_json::from_slice(&fs::read(&result.manifest).expect("manifest"))
        .expect("JSON manifest");
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["patient_id"], "patient-a");
    assert_eq!(manifest["members"].as_array().expect("membri").len(), 2);
    assert!(manifest["warnings"].as_array().expect("warning").is_empty());
}

#[test]
fn container_trace_records_archive_boundary_and_keeps_events_terse() {
    let directory = TestDirectory::new();
    let base_person = person(directory.path());
    fs::create_dir_all(&base_person.source_documents).expect("directory documenti");
    #[cfg(unix)]
    let archive_name = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"private-\xff.zip".to_vec())
    };
    #[cfg(not(unix))]
    let archive_name = std::ffi::OsString::from("private.zip");

    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let mut config = base_person.clone();
        config.local_build = directory.path().join(format!("build-{suffix}"));
        let archive = config.source_documents.join(&archive_name);
        write_zip(
            &archive,
            &[("private/report.txt", b"Synthetic private archive member")],
        );
        let inventory = source_inventory(&config);
        let logs = directory.path().join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        let result = session
            .scoped(|| stage_container_documents(&config, &inventory))
            .expect("staging archivio sintetico");
        assert_eq!(result.documents.len(), 1);
        assert_eq!(
            fs::read(&result.documents[0].path).expect("membro estratto"),
            b"Synthetic private archive member"
        );
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("containers::stage_container_documents"));
            assert!(log.contains("containers::extract_container"));
            assert!(log.contains("containers::extract_zip"));
            assert!(log.contains("archive_payload"));
            assert!(log.contains("member_contents"));
            assert!(log.contains("external_component"));
            assert!(log.contains("members"));
            assert!(log.contains("sha256"));
            #[cfg(unix)]
            assert!(log.contains("unix_bytes"));
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("Synthetic private archive member"));
            assert!(!log.contains("private-"));
        }
    }
}

#[test]
fn trace_redacts_secret_assignments_in_member_names_and_records_impl_spans() {
    let directory = TestDirectory::new();
    let mut config = person(directory.path());
    config.local_build = directory.path().join("build-trace");
    fs::create_dir_all(&config.source_documents).expect("directory documenti sintetica");
    let archive = config.source_documents.join("private.zip");
    let secret = "SECRET-SYNTHETIC-MEMBER";
    let member_name = format!("api_key={secret}/report.txt");
    write_zip(&archive, &[("report.txt", b"contenuto sintetico")]);
    let inventory = source_inventory(&config);
    let logs = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione trace sintetica");

    let result = session
        .scoped(|| {
            let listing = format!("----------\nPath = {member_name}\nSize = 19\n");
            super::parse_7z_listing(&listing);
            let _ = safe_member_path(&config.local_build, &member_name);
            stage_container_documents(&config, &inventory)
        })
        .expect("staging ZIP sintetico");
    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        result.documents[0].internal_path.as_deref(),
        Some("report.txt")
    );
    assert_eq!(
        fs::read(&result.documents[0].path).unwrap(),
        b"contenuto sintetico"
    );
    drop(session);

    let log = fs::read_to_string(log_path).expect("log trace sintetico");
    assert!(!log.contains(secret));
    assert!(log.contains("secret_omitted"));
    for function in [
        "containers::stage_container_documents_impl",
        "containers::extract_container_impl",
        "containers::extract_zip_impl",
        "containers::parse_7z_listing_impl",
        "containers::safe_member_path_impl",
        "containers::checked_output_path",
        "containers::write_member",
        "containers::create_member",
        "containers::regular_files",
        "containers::member_record",
        "containers::write_manifest",
    ] {
        assert!(log.contains(function), "span mancante: {function}");
    }
}

#[test]
fn ingestion_config_trace_redacts_secret_assignments_and_keeps_private_values() {
    let directory = TestDirectory::new();
    let ingestion = IngestionConfig {
        exclude_patterns: vec!["api_key=synthetic-ingestion-secret".to_owned()],
        include_patterns: vec!["private-pattern-synthetic".to_owned()],
    };
    let document = DocumentRecord {
        document_id: "document-synthetic".to_owned(),
        patient_id: "patient-synthetic".to_owned(),
        path: directory.path().join("document.txt"),
        title: "Titolo privato sintetico".to_owned(),
        category: "uncategorized".to_owned(),
        kind: "text".to_owned(),
        sha256: "digest-synthetic".to_owned(),
        date: None,
        series: None,
        tags: Vec::new(),
        origin: "source".to_owned(),
        container_id: None,
        internal_path: None,
    };
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");

    assert!(session.scoped(|| super::should_ingest_staged_document(&ingestion, &document)));
    drop(session);

    let contents = fs::read_to_string(log_path).expect("log sintetico");
    assert!(
        contents.contains("private-pattern-synthetic"),
        "ordinary private pattern missing from trace: {contents}"
    );
    assert!(contents.contains("Titolo privato sintetico"));
    assert!(contents.contains("secret_omitted"));
    assert!(!contents.contains("synthetic-ingestion-secret"));
}

#[test]
fn writes_an_empty_manifest_when_the_inventory_has_no_containers() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(
        person.source_documents.join("report.txt"),
        b"Synthetic report",
    )
    .expect("documento sintetico");

    let result =
        stage_container_documents(&person, &source_inventory(&person)).expect("manifest vuoto");

    assert!(result.documents.is_empty());
    assert!(result.members.is_empty());
    assert!(result.warning_messages.is_empty());
    let manifest: Value = serde_json::from_slice(&fs::read(result.manifest).expect("manifest"))
        .expect("JSON manifest");
    assert!(manifest["members"].as_array().expect("membri").is_empty());
}

#[test]
fn rejects_zip_traversal_without_writing_outside_the_staging_root() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    write_zip(
        &person.source_documents.join("unsafe.zip"),
        &[("../../escape.txt", b"must not be written")],
    );

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("errore del singolo archivio come warning");

    assert!(result.documents.is_empty());
    assert!(result.members.is_empty());
    assert_eq!(result.warning_messages.len(), 1);
    assert!(!directory.path().join("escape.txt").exists());
}

#[test]
fn recursively_stages_nested_archives_once_and_applies_ingestion_filters() {
    let directory = TestDirectory::new();
    let mut person = person(directory.path());
    person.ingestion.exclude_patterns = vec!["*.txt".to_owned()];
    person.ingestion.include_patterns = vec!["keep.txt".to_owned()];
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let nested = zip_bytes(&[("KEEP.TXT", b"included"), ("skip.txt", b"excluded")]);
    write_zip(
        &person.source_documents.join("outer.zip"),
        &[("nested.zip", &nested), ("outer.txt", b"excluded")],
    );

    let result =
        stage_container_documents(&person, &source_inventory(&person)).expect("staging ricorsivo");

    assert_eq!(result.nested_containers.len(), 1);
    assert_eq!(result.members.len(), 4);
    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        result.documents[0].internal_path.as_deref(),
        Some("KEEP.TXT")
    );
    assert_eq!(
        result.documents[0]
            .path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("KEEP.TXT")
    );
}

#[test]
fn skips_repeated_container_documents_by_content_identity() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    write_zip(
        &person.source_documents.join("archive.zip"),
        &[("report.txt", b"Synthetic report")],
    );
    let mut inventory = source_inventory(&person);
    inventory.push(inventory[0].clone());

    let result = stage_container_documents(&person, &inventory).expect("container deduplicato");

    assert_eq!(result.members.len(), 1);
    assert_eq!(result.documents.len(), 1);
}

#[test]
fn tar_xz_stages_regular_members_and_skips_links() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let archive_path = person.source_documents.join("records.tar.xz");
    let mut tar_bytes = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(13);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, "records/report.txt", &b"Synthetic TXT"[..])
            .expect("membro TAR sintetico");
        archive.finish().expect("TAR completo");
    }
    let midpoint = tar_bytes.len() / 2;
    let mut compressed = Vec::new();
    for chunk in [&tar_bytes[..midpoint], &tar_bytes[midpoint..]] {
        let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 6);
        encoder.write_all(chunk).expect("stream XZ sintetico");
        compressed.extend(encoder.finish().expect("stream XZ completo"));
    }
    fs::write(&archive_path, compressed).expect("archivio TAR.XZ sintetico");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging TAR.XZ sintetico");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro TAR estratto"),
        b"Synthetic TXT"
    );
}

#[test]
fn stages_7z_members_with_the_validated_extraction_callback() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let input = directory.path().join("input");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::create_dir_all(&input).expect("directory sorgente 7z");
    fs::write(input.join("report.txt"), b"Synthetic 7z report").expect("membro sintetico");
    let archive = person.source_documents.join("records.7z");
    sevenz_rust2::compress_to_path(&input, &archive).expect("archivio 7z sintetico");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging 7z sintetico");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro 7z estratto"),
        b"Synthetic 7z report"
    );
}

#[test]
fn disk_image_suffix_uses_external_listing_and_streamed_member_extraction() {
    if !command_available("7zz") && !command_available("7z") {
        return;
    }
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let input = directory.path().join("image-input");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::create_dir_all(&input).expect("directory immagine sintetica");
    fs::write(input.join("report.txt"), b"Synthetic image member").expect("membro sintetico");
    let image = person.source_documents.join("sample.iso");
    sevenz_rust2::compress_to_path(&input, &image).expect("archivio sintetico col suffisso ISO");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging del percorso immagini");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro immagine estratto"),
        b"Synthetic image member"
    );
}

#[test]
fn recursively_stages_nested_disk_image_members() {
    if !command_available("7zz") && !command_available("7z") {
        return;
    }
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let nested_source = directory.path().join("nested-image");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::create_dir_all(nested_source.join("DICOM")).expect("directory DICOM sintetica");
    let mut dicom = vec![0; 128];
    dicom.extend_from_slice(b"DICM");
    fs::write(nested_source.join("DICOM/IM000001.dcm"), dicom).expect("DICOM sintetico");
    let nested_image = directory.path().join("study-image.iso");
    sevenz_rust2::compress_to_path(&nested_source, &nested_image)
        .expect("contenitore 7z sintetico con suffisso ISO");
    write_zip(
        &person.source_documents.join("study.zip"),
        &[(
            "Study.iso",
            &fs::read(nested_image).expect("immagine sintetica"),
        )],
    );

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging annidato ZIP e immagine");

    assert_eq!(result.warning_messages, Vec::<String>::new());
    assert_eq!(result.documents.len(), 2);
    assert_eq!(
        result.documents[0].internal_path.as_deref(),
        Some("Study.iso")
    );
    assert_eq!(
        result.documents[1].internal_path.as_deref(),
        Some("DICOM/IM000001.dcm")
    );
    assert_eq!(result.documents[1].kind, "dicom_file");
}

#[test]
fn invalid_rar_is_reported_without_aborting_the_manifest() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(person.source_documents.join("broken.rar"), b"not a RAR")
        .expect("contenitore RAR corrotto sintetico");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("warning RAR non fatale");

    assert!(result.documents.is_empty());
    assert!(result.members.is_empty());
    assert_eq!(result.warning_messages.len(), 1);
}

#[test]
fn stages_rar_members_through_stdout_without_using_archive_paths() {
    if !command_available("unrar") {
        return;
    }
    let Some(rar) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(if cfg!(windows) { "rar.exe" } else { "rar" }))
            .find(|candidate| candidate.is_file())
    }) else {
        return;
    };
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let input = directory.path().join("rar-input.txt");
    fs::write(&input, b"Synthetic RAR report").expect("membro RAR sintetico");
    let archive = person.source_documents.join("records.rar");
    let created = std::process::Command::new(rar)
        .args(["a", "-idq"])
        .arg(&archive)
        .arg(&input)
        .output()
        .expect("creazione RAR sintetico");
    assert!(
        created.status.success(),
        "RAR sintetico: {:?}",
        created.status
    );

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging RAR sintetico");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro RAR estratto"),
        b"Synthetic RAR report"
    );
}

#[test]
fn member_paths_reject_posix_and_windows_traversal() {
    let root = Path::new("/staging/container");
    for name in [
        "/etc/passwd",
        "../../escape.txt",
        "..\\..\\escape.txt",
        "C:\\outside\\file.txt",
        "D:/outside.txt",
    ] {
        assert!(safe_member_path(root, name).is_err(), "{name}");
    }
    assert!(safe_member_path(root, "reports/file.txt").is_ok());
    if cfg!(windows) {
        for name in ["CON", "NUL.txt", "COM1.log", "report:stream", "trailing. "] {
            assert!(safe_member_path(root, name).is_err(), "{name}");
        }
    }
    assert!(is_container_path(Path::new("case.TAR.XZ")));
    assert!(is_container_path(Path::new("nested.ISO")));
    assert!(!is_container_path(Path::new("document.pdf")));
}

#[test]
fn corrupt_archive_emits_warning_and_still_writes_empty_manifest() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(person.source_documents.join("broken.zip"), b"not a ZIP")
        .expect("contenitore corrotto sintetico");

    let result =
        stage_container_documents(&person, &source_inventory(&person)).expect("warning non fatale");

    assert!(result.documents.is_empty());
    assert!(result.members.is_empty());
    assert_eq!(result.warning_messages.len(), 1);
    assert!(result.manifest.is_file());
}
