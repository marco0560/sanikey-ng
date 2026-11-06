use super::{BuildMode, BuildOptions, build_all, build_patient};
use crate::config::{
    AccountsConfig, IngestionConfig, PersonConfig, SearchConfig, UiConfig, UsbConfig,
};
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
