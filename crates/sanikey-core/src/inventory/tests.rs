//! Fixture sintetiche per scansione, classificazione, digest e filtri.

use super::{find_duplicate_documents, scan_document_inventory, scan_documents, sha256_file};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::logging::{Component, Level, Session};
use crate::metadata::DocumentRecord;
use crate::operation::{ErrorCode, OperationError};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Gestisce la root temporanea usata dalle fixture d'inventario.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-inventory-{}-{}",
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

/// Rimuove i file della fixture quando il test termina.
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

fn traced_path(path: &Path) -> Value {
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
            "value": {"encoding": "unix_bytes", "value": path.as_os_str().as_bytes()},
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

fn traced_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> Value {
    json!({
        "kind": "data",
        "value": {
            "format": "rust_debug",
            "value": crate::logging::redact_secret_assignments(&format!("{value:#?}")),
        },
    })
}

fn traced_argument(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn trace_document_paths(documents: &[DocumentRecord]) -> Value {
    json!({
        "kind": "data",
        "value": documents
            .iter()
            .enumerate()
            .map(|(index, document)| json!({
                "index": index,
                "document_id": document.document_id,
                "path": traced_path(&document.path),
            }))
            .collect::<Vec<_>>(),
    })
}

fn trace_person_paths(person: &PersonConfig) -> Value {
    json!({
        "kind": "data",
        "value": [
            {"name": "source_documents", "path": traced_path(&person.source_documents)},
            {"name": "metadata_directory", "path": traced_path(&person.metadata_directory)},
            {"name": "local_build", "path": traced_path(&person.local_build)},
            {"name": "ui.background_image", "path": null},
            {"name": "search.dictionary", "path": null},
        ],
    })
}

fn trace_operation_exit(debug_result: &str, native_paths: Value) -> Value {
    json!({
        "kind": "data",
        "value": {
            "format": "rust_debug",
            "value": crate::logging::redact_secret_assignments(debug_result),
            "native_paths": native_paths,
        },
    })
}

fn assert_inventory_spans(records: &[Value], function: &str, mut expected: Vec<(Value, Value)>) {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), expected.len(), "entrate di {function}");
    let mut unmatched = entries;
    for (arguments, result) in expected.drain(..) {
        let entry_index = unmatched
            .iter()
            .position(|entry| entry["arguments"] == arguments)
            .unwrap_or_else(|| panic!("argomenti attesi assenti per {function}: {arguments}"));
        let entry = unmatched.remove(entry_index);
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
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
    assert!(unmatched.is_empty());
}

#[test]
fn inventory_error_trace_redacts_secret_like_debug_text_without_changing_error() {
    let directory = TestDirectory::new();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");
    let message = "password = synthetic-inventory-secret";
    let error = session.scoped(|| {
        super::filesystem_error(
            message,
            Path::new("synthetic.txt"),
            std::io::Error::other("errore sintetico"),
        )
    });
    assert!(error.message().contains(message));
    drop(session);

    let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
    assert!(log.contains("secret_omitted"));
    assert!(!log.contains("synthetic-inventory-secret"));
}

#[test]
fn inventory_creates_stable_records_and_preserves_original_bytes() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let category = person.source_documents.join("laboratory");
    fs::create_dir_all(&category).expect("categoria sintetica");
    let path = category.join("20260102 Blood_Test.txt");
    let original = b"Synthetic text";
    fs::write(&path, original).expect("documento sintetico");

    let inventory = scan_document_inventory(&person).expect("inventario sintetico");

    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].patient_id, "patient-a");
    assert_eq!(inventory[0].category, "laboratory");
    assert_eq!(inventory[0].date.as_deref(), Some("2026-01-02"));
    assert_eq!(inventory[0].title, "Blood Test");
    assert_eq!(inventory[0].kind, "text");
    assert_eq!(inventory[0].sha256.len(), 64);
    assert_eq!(
        inventory[0].sha256,
        "6a34655cc94a45249136d8901d7ea1d4faf083962b421f568aa8120d51e83c29"
    );
    assert_eq!(inventory[0].document_id, inventory[0].sha256);
    assert_eq!(inventory[0].origin, "source");
    assert_eq!(fs::read(path).expect("originale invariato"), original);
}

#[test]
fn migration_corpus_inventory_matches_the_python_baseline() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/migration/corpus/minimal/patient-a/documents");
    let mut person = person(Path::new("."));
    person.source_documents = fixture_root.canonicalize().expect("fixture canonica");
    let original_bytes: Vec<_> = [
        "duplicate/20260102 Copia sintetica.txt",
        "laboratory/20260102 Referto Sintetico.txt",
        "laboratory/peso-sintetico.csv",
    ]
    .map(|relative| {
        let path = person.source_documents.join(relative);
        (
            path.clone(),
            fs::read(path).expect("byte originali della fixture"),
        )
    })
    .into();

    let inventory = scan_document_inventory(&person).expect("inventario del corpus");
    let paths: Vec<_> = inventory
        .iter()
        .map(|record| {
            record
                .path
                .strip_prefix(&person.source_documents)
                .expect("percorso relativo alla root")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    let duplicates = find_duplicate_documents(&inventory);
    let unique = scan_documents(&person).expect("documenti distinti del corpus");

    assert_eq!(inventory.len(), 3);
    assert_eq!(
        paths,
        [
            "duplicate/20260102 Copia sintetica.txt",
            "laboratory/20260102 Referto Sintetico.txt",
            "laboratory/peso-sintetico.csv",
        ]
    );
    assert_eq!(
        inventory[0].sha256,
        "f1c69fd460ac2e3f88c30c5fca12ea77adb1a06660d053dafcf2e19f642934f7"
    );
    assert_eq!(inventory[1].sha256, inventory[0].sha256);
    assert_eq!(inventory[1].category, "laboratory");
    assert_eq!(inventory[1].date.as_deref(), Some("2026-01-02"));
    assert_eq!(inventory[1].title, "Referto Sintetico");
    assert_eq!(
        inventory[2].sha256,
        "44310ca2b3502659a16103dc7a592f86159f403e4219108fa6daad6742586f6d"
    );
    assert_eq!(inventory[2].kind, "binary");
    assert_eq!(duplicates.len(), 1);
    assert_eq!(unique.len(), 2);
    assert!(
        original_bytes
            .into_iter()
            .all(|(path, bytes)| fs::read(path).expect("byte finali fixture") == bytes)
    );
}

#[test]
fn inventory_classifies_service_categories_compound_suffixes_and_dicom_magic() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let source = &person.source_documents;
    fs::create_dir_all(source.join("_Parametri")).expect("categoria parametri");
    fs::create_dir_all(source.join("_Archivi")).expect("categoria archivi");
    fs::write(source.join("20260101 Study.img"), b"synthetic img").expect("immagine DICOM");
    fs::write(source.join("_Parametri/20260102 Pressione.txt"), b"text").expect("file testo");
    fs::write(source.join("_Archivi/20260103 Studio.iso"), b"iso").expect("file iso");
    fs::write(source.join("20260104 Support.tar.xz"), b"archive").expect("archivio");
    fs::write(source.join("20260105 Report.docx"), b"office").expect("file Office");
    fs::write(source.join("20260106 Workbook.xlsx"), b"spreadsheet").expect("file XLSX");
    fs::write(source.join("20260107 Photo.PNG"), b"image").expect("immagine");
    fs::write(source.join("20260108 Digital.pdf"), b"pdf").expect("PDF");
    fs::write(source.join("plain.txt"), b"root-level").expect("file senza categoria");
    fs::write(
        source.join("IM000001"),
        [vec![0; 128], b"DICM".to_vec(), b"synthetic".to_vec()].concat(),
    )
    .expect("file DICOM sintetico");

    let inventory = scan_document_inventory(&person).expect("inventario sintetico");
    let records: std::collections::BTreeMap<_, _> = inventory
        .iter()
        .map(|record| (record.path.file_name().unwrap().to_string_lossy(), record))
        .collect();

    assert_eq!(records["20260102 Pressione.txt"].category, "Parametri");
    assert_eq!(records["20260103 Studio.iso"].category, "Archivi");
    assert_eq!(records["20260103 Studio.iso"].kind, "dicom_iso");
    assert_eq!(records["20260101 Study.img"].kind, "dicom_img");
    assert_eq!(records["20260104 Support.tar.xz"].kind, "archive");
    assert_eq!(records["20260104 Support.tar.xz"].title, "Support");
    assert_eq!(records["20260105 Report.docx"].kind, "office");
    assert_eq!(records["20260106 Workbook.xlsx"].kind, "office");
    assert_eq!(records["20260107 Photo.PNG"].kind, "image");
    assert_eq!(records["20260108 Digital.pdf"].kind, "pdf");
    assert_eq!(records["plain.txt"].category, "uncategorized");
    assert_eq!(records["IM000001"].kind, "dicom_file");
}

#[test]
fn include_patterns_recover_excluded_files_before_hashing() {
    let directory = TestDirectory::new();
    let mut person = person(directory.path());
    person.ingestion = IngestionConfig {
        exclude_patterns: vec!["**/Help/**".to_owned(), "*.tmp".to_owned()],
        include_patterns: vec![
            "**/Help/keep.txt".to_owned(),
            "**/Help/keep[0-9].txt".to_owned(),
        ],
    };
    let source = &person.source_documents;
    fs::create_dir_all(source.join("Help")).expect("directory esclusa");
    fs::create_dir_all(source.join("Nested")).expect("directory nested");
    fs::write(source.join("report.txt"), b"included").expect("file incluso");
    fs::write(source.join("Help/manual.txt"), b"excluded").expect("file escluso");
    fs::write(source.join("Help/keep.txt"), b"recovered").expect("file recuperato");
    fs::write(source.join("Help/keep7.txt"), b"recovered by range").expect("classe glob");
    fs::write(source.join("scratch.tmp"), b"excluded").expect("file temporaneo");
    fs::write(source.join("Nested/OTHER.TMP"), b"excluded").expect("pattern case-insensitive");

    let inventory = scan_document_inventory(&person).expect("inventario filtrato");
    let names: Vec<_> = inventory
        .iter()
        .map(|record| {
            record
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    assert_eq!(names, ["keep.txt", "keep7.txt", "report.txt"]);
}

#[test]
fn duplicate_documents_are_grouped_and_first_sorted_path_is_retained() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let source = &person.source_documents;
    fs::create_dir_all(source.join("reports")).expect("directory report");
    let first = source.join("reports/20260102 A.txt");
    let second = source.join("reports/20260103 B.txt");
    fs::write(&first, b"identical synthetic bytes").expect("primo originale");
    fs::write(&second, b"identical synthetic bytes").expect("secondo originale");

    let inventory = scan_document_inventory(&person).expect("inventario con duplicati");
    let unique = scan_documents(&person).expect("documenti deduplicati");
    let duplicates = find_duplicate_documents(&inventory);

    assert_eq!(inventory.len(), 2);
    assert_eq!(unique.len(), 1);
    assert_eq!(unique[0].path, first);
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates.values().next().unwrap().len(), 2);
    assert_eq!(
        fs::read(source.join("reports/20260102 A.txt")).expect("primo originale invariato"),
        b"identical synthetic bytes"
    );
    assert_eq!(
        fs::read(source.join("reports/20260103 B.txt")).expect("secondo originale invariato"),
        b"identical synthetic bytes"
    );
}

#[test]
fn missing_or_non_directory_source_roots_produce_empty_inventories() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    assert!(
        scan_document_inventory(&person)
            .expect("directory mancante")
            .is_empty()
    );

    fs::write(&person.source_documents, b"not a directory").expect("file sintetico");
    assert!(
        scan_document_inventory(&person)
            .expect("root non-directory")
            .is_empty()
    );
}

#[test]
fn digest_read_errors_keep_a_configuration_error_and_path_context() {
    let directory = TestDirectory::new();
    let path = directory.path().join("documento-assente.txt");

    let error = sha256_file(&path).expect_err("file mancante");

    assert_eq!(error.code(), ErrorCode::Configuration);
    assert!(error.message().contains("documento-assente.txt"));
}

#[cfg(unix)]
#[test]
fn inventory_trace_preserves_non_unicode_paths_and_is_off_at_events_level() {
    use std::os::unix::ffi::OsStringExt;

    use crate::logging::{Component, Level, Session};

    for level in [Level::Events, Level::Trace] {
        let directory = TestDirectory::new();
        let person = person(directory.path());
        fs::create_dir_all(&person.source_documents).expect("root documenti sintetico");
        let weird_name = std::ffi::OsString::from_vec(b"nonunicode-\xff.txt".to_vec());
        fs::write(person.source_documents.join(weird_name), b"synthetic").expect("file sintetico");
        fs::write(
            person.source_documents.join("duplicate-a.txt"),
            b"same bytes",
        )
        .expect("primo duplicato sintetico");
        fs::write(
            person.source_documents.join("duplicate-b.txt"),
            b"same bytes",
        )
        .expect("secondo duplicato sintetico");
        fs::create_dir_all(person.source_documents.join("nested"))
            .expect("directory annidata sintetica");
        fs::write(
            person.source_documents.join("nested/extra.txt"),
            b"nested bytes",
        )
        .expect("file annidato sintetico");
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.path().join("logs"),
            Component::Core,
            level,
        )
        .expect("sessione sintetica");

        let (documents, duplicates) = session.scoped(|| {
            let documents = scan_document_inventory(&person).expect("inventario sintetico");
            let duplicates = find_duplicate_documents(&documents);
            (documents, duplicates)
        });
        assert_eq!(documents.len(), 4);
        assert_eq!(duplicates.len(), 1);
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
        if level == Level::Trace {
            assert!(log.contains("inventory::scan_document_inventory"));
            assert!(log.contains("inventory::collect_files"));
            assert!(log.contains("inventory::document_record"));
            assert!(log.contains("inventory::find_duplicate_documents"));
            assert!(log.contains("unix_bytes"));
            assert!(log.contains("nonunicode-"));
            let records = log
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("JSON valido"))
                .collect::<Vec<_>>();
            let calls = records
                .iter()
                .filter(|record| {
                    record["type"] == "function_enter"
                        && record["function"] == "inventory::collect_files"
                })
                .collect::<Vec<_>>();
            assert!(calls.iter().any(|record| {
                record["arguments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|argument| {
                        argument["name"] == "depth" && argument["content"]["value"]["value"] == "0"
                    })
            }));
            assert!(calls.iter().any(|record| {
                record["arguments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|argument| {
                        argument["name"] == "depth" && argument["content"]["value"]["value"] == "1"
                    })
            }));
            for call in calls {
                let exit = records
                    .iter()
                    .find(|record| {
                        record["type"] == "function_exit" && record["call_id"] == call["call_id"]
                    })
                    .expect("uscita correlata via call_id");
                assert!(exit["result"]["value"]["value"]["files_length_after"].is_u64());
            }
        } else {
            assert!(!log.contains("inventory::"));
            assert!(!log.contains("patient-a"));
        }
    }
}

#[test]
fn synthetic_inventory_spans_record_arguments_results_durations_and_redactions() {
    let directory = TestDirectory::new();
    let scan_person = person(&directory.path().join("scan"));
    fs::create_dir_all(&scan_person.source_documents).expect("root scansione vuota");

    let direct_root = directory.path().join("direct");
    fs::create_dir_all(&direct_root).expect("root documento diretto");
    let direct_path = direct_root.join("20260102 Direct report.txt");
    fs::write(&direct_path, b"direct synthetic bytes").expect("documento diretto sintetico");

    let staged_root = directory.path().join("staging");
    fs::create_dir_all(staged_root.join("archive")).expect("root staging sintetico");
    let staged_path = staged_root.join("archive/20260103 Staged report.txt");
    fs::write(&staged_path, b"staged synthetic bytes").expect("documento staging sintetico");

    let checksum_path = directory.path().join("checksum-only.bin");
    fs::write(&checksum_path, b"checksum synthetic bytes").expect("file checksum sintetico");

    let documents = vec![
        DocumentRecord {
            document_id: "shared-digest".to_owned(),
            patient_id: "patient-a".to_owned(),
            path: directory.path().join("duplicates/a.txt"),
            title: "A".to_owned(),
            category: "reports".to_owned(),
            kind: "text".to_owned(),
            sha256: "shared-digest".to_owned(),
            date: None,
            series: None,
            tags: Vec::new(),
            origin: "source".to_owned(),
            container_id: None,
            internal_path: None,
        },
        DocumentRecord {
            document_id: "shared-digest".to_owned(),
            patient_id: "patient-a".to_owned(),
            path: directory.path().join("duplicates/b.txt"),
            title: "B".to_owned(),
            category: "reports".to_owned(),
            kind: "text".to_owned(),
            sha256: "shared-digest".to_owned(),
            date: None,
            series: None,
            tags: Vec::new(),
            origin: "source".to_owned(),
            container_id: None,
            internal_path: None,
        },
    ];

    let mut filtered_person = person(directory.path());
    filtered_person.ingestion = IngestionConfig {
        include_patterns: Vec::new(),
        exclude_patterns: vec!["**/*.tmp".to_owned()],
    };
    let excluded_path = "excluded/report.tmp";
    let secret = "synthetic-inventory-secret";
    let error_action = format!("password = {secret}");
    let cause = std::io::Error::other(format!("api_key = {secret}"));

    let (
        inventory,
        unique,
        duplicates,
        direct_record,
        staged_record,
        excluded,
        checksum,
        error,
        contents,
    ) = {
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.path().join("logs"),
            Component::Core,
            Level::Trace,
        )
        .expect("sessione trace sintetica");
        let results = session.scoped(|| {
            (
                scan_document_inventory(&scan_person).expect("inventario vuoto sintetico"),
                scan_documents(&scan_person).expect("documenti distinti vuoti sintetici"),
                find_duplicate_documents(&documents),
                super::document_record(direct_path.clone(), &direct_root, "patient-direct")
                    .expect("record diretto sintetico"),
                super::staged_document_record(
                    staged_path.clone(),
                    &staged_root,
                    "patient-staged",
                    "container-synthetic",
                )
                .expect("record staging sintetico"),
                super::is_ingestion_excluded(&filtered_person.ingestion, excluded_path),
                sha256_file(&checksum_path).expect("digest sintetico"),
                super::filesystem_error(&error_action, &direct_path, cause),
            )
        });
        let (
            inventory,
            unique,
            duplicates,
            direct_record,
            staged_record,
            excluded,
            checksum,
            error,
        ) = results;
        drop(session);
        let contents = fs::read_to_string(log_path).expect("log inventario sintetico");
        let records = contents
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("JSON valido"))
            .collect::<Vec<_>>();
        let no_paths = json!({"kind": "data", "value": []});
        let mut expected = Vec::new();

        let person_arguments = json!([
            traced_argument("person", traced_debug(&scan_person)),
            traced_argument("person_native_paths", trace_person_paths(&scan_person)),
        ]);
        let empty_result = Ok::<Vec<DocumentRecord>, OperationError>(Vec::new());
        let empty_native_paths = json!({
            "kind": "data",
            "value": trace_document_paths(&[]),
        });
        let empty_exit = trace_operation_exit(&format!("{empty_result:#?}"), empty_native_paths);
        expected.push((person_arguments.clone(), empty_exit.clone()));
        expected.push((person_arguments, empty_exit));
        assert_inventory_spans(&records, "inventory::scan_document_inventory", expected);

        assert_inventory_spans(
            &records,
            "inventory::scan_documents",
            vec![(
                json!([
                    traced_argument("person", traced_debug(&scan_person)),
                    traced_argument("person_native_paths", trace_person_paths(&scan_person)),
                ]),
                trace_operation_exit(
                    &format!("{empty_result:#?}"),
                    json!({
                        "kind": "data",
                        "value": trace_document_paths(&[]),
                    }),
                ),
            )],
        );

        let duplicate_native_paths = json!({
            "kind": "data",
            "value": [{
                "digest": "shared-digest",
                "documents": trace_document_paths(&documents),
            }],
        });
        assert_inventory_spans(
            &records,
            "inventory::find_duplicate_documents",
            vec![(
                json!([
                    traced_argument("documents", traced_debug(&documents)),
                    traced_argument("document_native_paths", trace_document_paths(&documents)),
                ]),
                trace_operation_exit(&format!("{duplicates:#?}"), duplicate_native_paths),
            )],
        );

        let direct_result = Ok::<DocumentRecord, OperationError>(direct_record.clone());
        let mut staged_source_record = staged_record.clone();
        staged_source_record.origin = "source".to_owned();
        staged_source_record.container_id = None;
        staged_source_record.internal_path = None;
        let staged_source_result = Ok::<DocumentRecord, OperationError>(staged_source_record);
        let staged_result = Ok::<DocumentRecord, OperationError>(staged_record.clone());
        assert_inventory_spans(
            &records,
            "inventory::document_record",
            vec![
                (
                    json!([
                        traced_argument("patient_id", traced_debug(&"patient-direct")),
                        traced_argument("path", traced_path(&direct_path)),
                        traced_argument("root", traced_path(&direct_root)),
                    ]),
                    trace_operation_exit(
                        &format!("{direct_result:#?}"),
                        json!({
                            "kind": "data",
                            "value": {"path": traced_path(&direct_path)},
                        }),
                    ),
                ),
                (
                    json!([
                        traced_argument("patient_id", traced_debug(&"patient-staged")),
                        traced_argument("path", traced_path(&staged_path)),
                        traced_argument("root", traced_path(&staged_root)),
                    ]),
                    trace_operation_exit(
                        &format!("{staged_source_result:#?}"),
                        json!({
                            "kind": "data",
                            "value": {"path": traced_path(&staged_path)},
                        }),
                    ),
                ),
            ],
        );
        assert_inventory_spans(
            &records,
            "inventory::staged_document_record",
            vec![(
                json!([
                    traced_argument("patient_id", traced_debug(&"patient-staged")),
                    traced_argument("container_id", traced_debug(&"container-synthetic")),
                    traced_argument("path", traced_path(&staged_path)),
                    traced_argument("root", traced_path(&staged_root)),
                ]),
                trace_operation_exit(
                    &format!("{staged_result:#?}"),
                    json!({
                        "kind": "data",
                        "value": {"path": traced_path(&staged_path)},
                    }),
                ),
            )],
        );

        assert_inventory_spans(
            &records,
            "inventory::is_ingestion_excluded",
            vec![(
                json!([
                    traced_argument("config", traced_debug(&filtered_person.ingestion)),
                    traced_argument("path", traced_debug(&excluded_path)),
                    traced_argument("path_native", traced_path(Path::new(excluded_path))),
                ]),
                trace_operation_exit(&format!("{excluded:#?}"), no_paths.clone()),
            )],
        );

        let expected_digest = sha256_file(&checksum_path).expect("digest atteso sintetico");
        let mut checksum_calls = Vec::new();
        for path in [&direct_path, &staged_path, &checksum_path] {
            let expected = Ok::<String, OperationError>(if path == &checksum_path {
                expected_digest.clone()
            } else {
                sha256_file(path).expect("digest record sintetico")
            });
            checksum_calls.push((
                json!([traced_argument("path", traced_path(path))]),
                trace_operation_exit(&format!("{expected:#?}"), no_paths.clone()),
            ));
        }
        assert_inventory_spans(&records, "inventory::sha256_file", checksum_calls);

        assert_inventory_spans(
            &records,
            "inventory::filesystem_error",
            vec![(
                json!([
                    traced_argument("action", traced_debug(&error_action)),
                    traced_argument(
                        "cause",
                        traced_debug(&std::io::Error::other(format!("api_key = {secret}")))
                    ),
                    traced_argument("path", traced_path(&direct_path)),
                ]),
                trace_operation_exit(&format!("{error:#?}"), no_paths),
            )],
        );
        (
            inventory,
            unique,
            duplicates,
            direct_record,
            staged_record,
            excluded,
            checksum,
            error,
            contents,
        )
    };

    assert!(inventory.is_empty());
    assert!(unique.is_empty());
    assert_eq!(duplicates.len(), 1);
    assert_eq!(direct_record.patient_id, "patient-direct");
    assert_eq!(direct_record.origin, "source");
    assert_eq!(staged_record.origin, "container");
    assert_eq!(
        staged_record.container_id.as_deref(),
        Some("container-synthetic")
    );
    assert!(excluded);
    assert_eq!(checksum.len(), 64);
    assert_eq!(error.code(), ErrorCode::Configuration);
    assert!(error.message().contains(secret));
    assert!(contents.contains("secret_omitted"));
    assert!(!contents.contains(secret));
}
