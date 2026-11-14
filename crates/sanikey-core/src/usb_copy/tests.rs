//! Fixture sintetiche per la copia verificata delle immagini USB.

#[cfg(target_os = "linux")]
use super::mirror_validated_txt_image_on_linux_volume;

use super::{
    empty_result, mirror_directory, mirror_validated_txt_image, normalize, resolve_path,
    trace_error, usb_error,
};
use crate::config::{
    AccountsConfig, IngestionConfig, PersonConfig, SearchConfig, UiConfig, UsbConfig,
};
use crate::operation::{
    CancellationToken, ErrorCode, OperationError, OperationEvent, OperationOutcome, OperationPhase,
};
use crate::simulated_export::{export_txt_simulation, validate_txt_simulation};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

/// Gestisce directory sorgente e destinazione temporanee dei test USB.
impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sanikey-usb-copy-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }
}

/// Pulisce le directory temporanee create dalle fixture.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(root: &Path) -> PersonConfig {
    PersonConfig {
        id: "synthetic-a".to_owned(),
        display_name: "Persona sintetica".to_owned(),
        source_documents: root.join("patients/synthetic-a/documents"),
        metadata_directory: root.join("patients/synthetic-a/metadata"),
        local_build: root.join("generated/synthetic-a"),
        usb_uuid: "SYNTHETIC-UUID".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

#[cfg(target_os = "linux")]
fn accounts_config(person: &PersonConfig) -> AccountsConfig {
    AccountsConfig {
        config_version: 1,
        people: vec![person.clone()],
        path: PathBuf::new(),
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
        usb: UsbConfig {
            usb_uuid: None,
            require_exfat: false,
            min_free_space_mb: 1,
            copy_strategy: "python".to_owned(),
        },
    }
}

fn read_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
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
            } else {
                files.push((
                    path.strip_prefix(root)
                        .expect("percorso figlio della radice")
                        .to_path_buf(),
                    fs::read(path).expect("byte sintetici leggibili"),
                ));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn trace_path(path: &Path) -> Value {
    if let Some(text) = path.to_str() {
        return json!({
            "kind": "data",
            "value": {"encoding": "utf8", "value": text},
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        json!({
            "kind": "data",
            "value": {
                "encoding": "unix_bytes",
                "value": path.as_os_str().as_bytes(),
            },
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        json!({
            "kind": "data",
            "value": {
                "encoding": "windows_utf16",
                "value": path.as_os_str().encode_wide().collect::<Vec<_>>(),
            },
        })
    }
}

fn trace_argument(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn assert_usb_trace(records: &[Value], function: &str, arguments: Value, result: Value) {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), 1, "una sola entrata per {function}");
    assert_eq!(
        entries[0]["arguments"], arguments,
        "argomenti di {function}"
    );
    let call_id = entries[0]["call_id"].as_u64().expect("call_id numerico");
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
    assert_eq!(exits[0]["result"], result, "risultato di {function}");
    assert!(
        exits[0]["duration_ms"].as_u64().is_some(),
        "durata mancante per {function}#{call_id}"
    );
}

#[test]
fn synthetic_trace_checks_usb_normalization_empty_result_and_secret_error() {
    use crate::logging::{Component, Level, Session};

    let temp = TestDirectory::new();
    let (session, log_path) = Session::create_in_directory_with_level(
        &temp.0.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");
    let normalize_input = temp.0.join("source/../target");
    let normalized = temp.0.join("target");
    let empty_root = temp.0.join("empty-image");
    let secret_message = "password = synthetic-usb-secret; dettaglio sintetico";
    let (normalized_result, empty, error) = session.scoped(|| {
        (
            normalize(&normalize_input),
            empty_result(empty_root.clone()),
            usb_error(secret_message),
        )
    });
    assert_eq!(normalized_result, normalized);
    assert_eq!(empty.root, empty_root);
    assert_eq!(empty.copied_files, 0);
    assert_eq!(empty.reused_files, 0);
    assert_eq!(empty.removed_files, 0);
    assert_eq!(empty.bytes_copied, 0);
    assert_eq!(error.code(), ErrorCode::Usb);
    assert_eq!(error.message(), secret_message);
    drop(session);

    let records: Vec<Value> = fs::read_to_string(&log_path)
        .expect("log USB sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect();
    assert_usb_trace(
        &records,
        "usb_copy::normalize",
        json!([trace_argument("path", trace_path(&normalize_input))]),
        json!({"kind": "data", "value": {"output": trace_path(&normalized)}}),
    );
    assert_usb_trace(
        &records,
        "usb_copy::empty_result",
        json!([trace_argument("root", trace_path(&empty_root))]),
        json!({
            "kind": "data",
            "value": {
                "output": {
                    "kind": "data",
                    "value": {
                        "root": trace_path(&empty_root),
                        "copied_files": 0,
                        "reused_files": 0,
                        "removed_files": 0,
                        "bytes_copied": 0,
                    },
                },
            },
        }),
    );
    let redacted_message = "password = [secret_omitted]; dettaglio sintetico";
    let trace_message = json!({
        "kind": "data",
        "value": {
            "format": "text_redacted_known_secret_assignments",
            "value": redacted_message,
        },
    });
    let traced_error = json!({
        "kind": "data",
        "value": {
            "state": "error",
            "code": "usb",
            "message": trace_message,
        },
    });
    assert_usb_trace(
        &records,
        "usb_copy::usb_error",
        json!([trace_argument("message", trace_message)]),
        json!({"kind": "data", "value": {"output": traced_error}}),
    );
    let serialized = serde_json::to_string(&records).expect("serializzazione record");
    assert!(serialized.contains("secret_omitted"));
    assert!(!serialized.contains("synthetic-usb-secret"));
}

#[test]
fn mirrors_a_manifest_verified_image_and_reuses_unchanged_files() {
    let temp = TestDirectory::new();
    let person = person(&temp.0);
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let original = person.source_documents.join("2026-01-02 report.txt");
    fs::write(&original, "Referto sintetico uno").expect("originale sintetico");
    let first_image = temp.0.join("image-first");
    export_txt_simulation(&person, &first_image).expect("prima immagine sintetica");
    let target = temp.0.join("target");
    let cancellation = CancellationToken::default();

    let first = mirror_validated_txt_image(&first_image, &target, &cancellation);
    let OperationOutcome::Succeeded(first_result) = first.outcome else {
        panic!("prima copia riuscita: {:?}", first.outcome);
    };
    assert!(first_result.copied_files > 0);
    assert_eq!(first_result.reused_files, 0);
    assert!(first.events.contains(&OperationEvent::Progress {
        phase: OperationPhase::Finalizing,
        completed: 1,
        total: Some(1),
    }));
    assert_eq!(read_tree(&first_image), read_tree(&target));
    validate_txt_simulation(&target).expect("manifest e checksum dopo la copia");
    let database = target.join("patients/synthetic-a/medical_archive.db");
    assert!(database.is_file());

    let second = mirror_validated_txt_image(&first_image, &target, &cancellation);
    let OperationOutcome::Succeeded(second_result) = second.outcome else {
        panic!("seconda copia riuscita: {:?}", second.outcome);
    };
    assert_eq!(second_result.copied_files, 0);
    assert_eq!(second_result.reused_files, first_result.copied_files);
    assert!(database.is_file());
    validate_txt_simulation(&target).expect("manifest stabile dopo il riuso");

    fs::write(target.join("obsolete.txt"), "file obsoleto sintetico")
        .expect("file obsoleto nel target");
    let cleaned = mirror_validated_txt_image(&first_image, &target, &cancellation);
    let OperationOutcome::Succeeded(cleaned_result) = cleaned.outcome else {
        panic!("rimozione file obsoleto riuscita: {:?}", cleaned.outcome);
    };
    assert_eq!(cleaned_result.removed_files, 1);
    validate_txt_simulation(&target).expect("manifest senza file obsoleto");

    fs::remove_file(&original).expect("rimozione del vecchio sorgente sintetico");
    fs::write(
        person.source_documents.join("2026-01-03 report.txt"),
        "Referto sintetico due",
    )
    .expect("nuovo originale sintetico");
    let second_image = temp.0.join("image-second");
    export_txt_simulation(&person, &second_image).expect("seconda immagine sintetica");
    let updated = mirror_validated_txt_image(&second_image, &target, &cancellation);
    let OperationOutcome::Succeeded(updated_result) = updated.outcome else {
        panic!("aggiornamento riuscito: {:?}", updated.outcome);
    };
    assert!(updated_result.copied_files > 0);
    assert_eq!(read_tree(&second_image), read_tree(&target));
    validate_txt_simulation(&target).expect("manifest e checksum aggiornati");
    assert_eq!(
        fs::read(person.source_documents.join("2026-01-03 report.txt")).unwrap(),
        b"Referto sintetico due"
    );
}

#[test]
fn validated_copy_repairs_target_tampering_and_rejects_invalid_source_without_changes() {
    let temp = TestDirectory::new();
    let person = person(&temp.0);
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(
        person.source_documents.join("2026-01-02 report.txt"),
        "referto sintetico",
    )
    .expect("originale sintetico");
    let image = temp.0.join("image");
    export_txt_simulation(&person, &image).expect("immagine valida");
    let target = temp.0.join("target");
    let cancellation = CancellationToken::default();

    let copied = mirror_validated_txt_image(&image, &target, &cancellation);
    assert!(matches!(copied.outcome, OperationOutcome::Succeeded(_)));
    validate_txt_simulation(&target).expect("target validato dopo la copia");

    let document = target.join("patients/synthetic-a/documents/2026-01-02 report.txt");
    fs::write(&document, "contenuto alterato").expect("mutazione target sintetica");
    let repaired = mirror_validated_txt_image(&image, &target, &cancellation);
    assert!(matches!(repaired.outcome, OperationOutcome::Succeeded(_)));
    validate_txt_simulation(&target).expect("target riparato e validato");

    fs::write(
        image.join("patients/synthetic-a/documents/2026-01-02 report.txt"),
        "immagine sorgente alterata",
    )
    .expect("mutazione immagine sorgente sintetica");
    let target_before = read_tree(&target);
    let rejected = mirror_validated_txt_image(&image, &target, &cancellation);
    assert!(matches!(rejected.outcome, OperationOutcome::Failed(_)));
    assert_eq!(read_tree(&target), target_before);
}

#[test]
fn validated_copy_reports_cancellation_without_success_or_target_creation() {
    let temp = TestDirectory::new();
    let person = person(&temp.0);
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(
        person.source_documents.join("2026-01-02 report.txt"),
        "referto sintetico",
    )
    .expect("originale sintetico");
    let image = temp.0.join("image");
    export_txt_simulation(&person, &image).expect("immagine valida");
    let target = temp.0.join("target");
    let cancellation = CancellationToken::default();
    cancellation.cancel();

    let cancelled = mirror_validated_txt_image(&image, &target, &cancellation);
    assert_eq!(cancelled.outcome, OperationOutcome::Cancelled);
    assert_eq!(cancelled.events.last(), Some(&OperationEvent::Cancelled));
    assert!(!target.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_volume_copy_validates_existing_mount_before_and_after_transfer() {
    let temp = TestDirectory::new();
    let person = person(&temp.0);
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(
        person.source_documents.join("2026-01-02 report.txt"),
        "referto sintetico",
    )
    .expect("originale sintetico");
    let image = temp.0.join("image");
    export_txt_simulation(&person, &image).expect("immagine valida");
    let config = accounts_config(&person);
    let target = temp.0.join("target");
    let rejected = mirror_validated_txt_image_on_linux_volume(
        &config,
        &image,
        &target,
        &CancellationToken::default(),
    );
    assert!(matches!(rejected.outcome, OperationOutcome::Failed(_)));
    assert!(!target.exists());

    fs::create_dir(&target).expect("directory volume sintetica esistente");
    let copied = mirror_validated_txt_image_on_linux_volume(
        &config,
        &image,
        &target,
        &CancellationToken::default(),
    );
    assert!(matches!(copied.outcome, OperationOutcome::Succeeded(_)));
    validate_txt_simulation(&target).expect("manifest e link verificati sul volume");
}

#[test]
fn rejects_unsafe_targets_before_modifying_them_and_recovers_after_correction() {
    let temp = TestDirectory::new();
    let source = temp.0.join("image");
    fs::create_dir_all(source.join("nested")).expect("directory sorgente");
    fs::write(source.join("nested/file.txt"), "byte sorgente sintetici")
        .expect("file sorgente sintetico");
    let target = temp.0.join("target");
    fs::create_dir(&target).expect("directory destinazione");
    fs::write(target.join("nested"), "preservare prima della validazione")
        .expect("conflitto sintetico");
    let source_before = read_tree(&source);
    let target_before = fs::read(target.join("nested")).expect("destinazione prima del tentativo");

    let failed = mirror_directory(&source, &target, &CancellationToken::default());
    assert!(matches!(failed.outcome, OperationOutcome::Failed(_)));
    assert_eq!(read_tree(&source), source_before);
    assert_eq!(fs::read(target.join("nested")).unwrap(), target_before);

    fs::remove_file(target.join("nested")).expect("rimozione del conflitto sintetico");
    fs::create_dir(target.join("nested")).expect("directory destinazione corretta");
    let resumed = mirror_directory(&source, &target, &CancellationToken::default());
    assert!(matches!(resumed.outcome, OperationOutcome::Succeeded(_)));
    assert_eq!(read_tree(&source), read_tree(&target));
    assert_eq!(read_tree(&source), source_before);
}

#[test]
fn reports_progress_and_honors_cancellation_before_copying() {
    let temp = TestDirectory::new();
    let source = temp.0.join("image");
    fs::create_dir(&source).expect("directory sorgente");
    fs::write(source.join("one.txt"), "contenuto sintetico").expect("file sorgente");
    let target = temp.0.join("target");
    let run = mirror_directory(&source, &target, &CancellationToken::default());
    assert!(matches!(run.outcome, OperationOutcome::Succeeded(_)));
    assert!(run.events.contains(&OperationEvent::Progress {
        phase: OperationPhase::Processing,
        completed: 1,
        total: Some(1),
    }));

    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let cancelled_target = temp.0.join("cancelled-target");
    let cancelled = mirror_directory(&source, &cancelled_target, &cancellation);
    assert_eq!(cancelled.outcome, OperationOutcome::Cancelled);
    assert!(!cancelled_target.exists());
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_in_source_and_destination() {
    use std::os::unix::fs::symlink;

    let temp = TestDirectory::new();
    let source = temp.0.join("image");
    fs::create_dir(&source).expect("directory immagine");
    let outside = temp.0.join("outside.txt");
    fs::write(&outside, "sorgente esterna").expect("file fuori immagine");
    symlink(&outside, source.join("link.txt")).expect("link sorgente sintetico");
    let target = temp.0.join("target");
    let result = mirror_directory(&source, &target, &CancellationToken::default());
    assert!(matches!(result.outcome, OperationOutcome::Failed(_)));
    assert!(!target.exists());

    fs::remove_file(source.join("link.txt")).expect("rimozione link sorgente");
    fs::create_dir(&target).expect("directory destinazione");
    let external_directory = temp.0.join("outside");
    fs::create_dir(&external_directory).expect("directory esterna");
    let target_link = temp.0.join("target-link");
    symlink(&external_directory, &target_link).expect("link destinazione sintetico");
    let result = mirror_directory(&source, &target_link, &CancellationToken::default());
    assert!(matches!(result.outcome, OperationOutcome::Failed(_)));
    assert!(read_tree(&external_directory).is_empty());
}

#[test]
fn rejects_source_target_overlap() {
    let temp = TestDirectory::new();
    let source = temp.0.join("image");
    fs::create_dir_all(source.join("nested")).expect("directory sorgente");
    let target_inside = source.join("target");
    let inside = mirror_directory(&source, &target_inside, &CancellationToken::default());
    assert!(matches!(inside.outcome, OperationOutcome::Failed(_)));
    assert!(!target_inside.exists());

    let target_parent = resolve_path(&source)
        .expect("sorgente risolta")
        .parent()
        .unwrap()
        .to_path_buf();
    let parent = mirror_directory(&source, &target_parent, &CancellationToken::default());
    assert!(matches!(parent.outcome, OperationOutcome::Failed(_)));
}

#[test]
fn synthetic_trace_records_copy_boundaries_and_events_omit_file_data() {
    use crate::logging::{Component, Level, Session};

    let temp = TestDirectory::new();
    let source = temp.0.join("synthetic-source");
    fs::create_dir_all(source.join("nested")).expect("albero sintetico");
    fs::write(
        source.join("nested/private-synthetic.txt"),
        b"synthetic private payload",
    )
    .expect("contenuto sintetico");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        fs::write(
            source.join(std::ffi::OsString::from_vec(
                b"non-unicode-\xff.bin".to_vec(),
            )),
            [3_u8, 5, 8],
        )
        .expect("file con nome non Unicode");
    }

    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let logs = temp.0.join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        let target = temp.0.join(format!("target-{suffix}"));
        let result =
            session.scoped(|| mirror_directory(&source, &target, &CancellationToken::default()));
        assert!(matches!(result.outcome, OperationOutcome::Succeeded(_)));
        assert_eq!(read_tree(&source), read_tree(&target));
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("usb_copy::mirror_directory"));
            assert!(log.contains("usb_copy::mirror_directory_inner"));
            assert!(log.contains("usb_copy::collect_source_tree"));
            assert!(log.contains("usb_copy::copy_file_atomically"));
            assert!(log.contains("usb_copy::sha256_file"));
            assert!(log.contains("private-synthetic.txt"));
            assert!(log.contains("expected_sha256"));
            assert!(log.contains("bytes_copied"));
            assert!(log.contains("file_contents_read"));
            assert!(log.contains("transferred_contents"));
            assert!(log.contains("external_component"));
            let payload_bytes = b"synthetic private payload"
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",");
            assert!(log.contains(&format!("[{payload_bytes}]")));
            assert!(!log.contains("synthetic private payload"));
            #[cfg(unix)]
            {
                assert!(log.contains("unix_bytes"));
                assert!(log.contains("255"));
            }
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("private-synthetic.txt"));
            assert!(!log.contains("synthetic private payload"));
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn validated_copy_trace_redacts_secret_assignments_in_config_debug() {
    use crate::logging::{Component, Level, Session};

    let temp = TestDirectory::new();
    let mut configured_person = person(&temp.0);
    configured_person.local_build = temp.0.join("generated/api_key=synthetic-usb-config-secret");
    let config = accounts_config(&configured_person);
    let (session, log_path) = Session::create_in_directory_with_level(
        &temp.0.join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione log sintetica");

    let result = session.scoped(|| {
        mirror_validated_txt_image_on_linux_volume(
            &config,
            temp.0.join("missing-source"),
            temp.0.join("missing-target"),
            &CancellationToken::default(),
        )
    });
    assert!(matches!(result.outcome, OperationOutcome::Failed(_)));
    drop(session);

    let contents = fs::read_to_string(log_path).expect("log sintetico");
    assert!(contents.contains("Persona sintetica"));
    assert!(contents.contains("secret_omitted"));
    assert!(!contents.contains("synthetic-usb-config-secret"));
}

#[test]
fn error_trace_redacts_known_assignments_and_keeps_other_private_message_text() {
    let original = "password = synthetic-usb-secret\npercorso privato sintetico";
    let error = OperationError::new(ErrorCode::Usb, original);
    let projection = trace_error(&error).to_json().to_string();

    assert!(!projection.contains("synthetic-usb-secret"));
    assert!(projection.contains("secret_omitted"));
    assert!(projection.contains("percorso privato sintetico"));
    assert_eq!(error.message(), original);
}
