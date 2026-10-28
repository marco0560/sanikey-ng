//! Fixture sintetiche per scansione, classificazione, digest e filtri.

use super::{find_duplicate_documents, scan_document_inventory, scan_documents, sha256_file};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::operation::ErrorCode;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

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
