use super::{build_pdf_pipeline, build_txt_pipeline};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::pdf::test_support::synthetic_pdf;
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-txt-pipeline-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
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

fn person(root: &Path, source: &Path, metadata: &Path) -> PersonConfig {
    PersonConfig {
        id: "patient-a".to_owned(),
        display_name: "Paziente sintetico A".to_owned(),
        source_documents: source.to_path_buf(),
        metadata_directory: metadata.to_path_buf(),
        local_build: root.join("generated"),
        usb_uuid: "1A2B-3C4D".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn txt_pipeline_matches_the_synthetic_baseline_and_builds_searchable_outputs() {
    let root = repository_root();
    let corpus = root.join("docs/migration/corpus/minimal/patient-a");
    let output = TestDirectory::new();
    let person = person(
        output.path(),
        &corpus.join("documents"),
        &corpus.join("metadata"),
    );
    let original_path = person
        .source_documents
        .join("duplicate/20260102 Copia sintetica.txt");
    let original_bytes = fs::read(&original_path).expect("fixture TXT sintetica");
    let result = build_txt_pipeline(&person).expect("pipeline TXT sintetica");

    assert_eq!(result.documents, 1);
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(root.join("docs/migration/corpus/expected/minimal-python.json"))
            .expect("expected della baseline fissata"),
    )
    .expect("JSON expected valido");
    let exported: Value =
        serde_json::from_str(&fs::read_to_string(&result.data).expect("JSON documenti esportato"))
            .expect("JSON esportato valido");
    let expected_document = &expected["positive"]["exported_documents"][0];
    let actual_document = &exported["documents"][0];
    for key in ["id", "path", "title", "category", "kind", "date", "tags"] {
        assert_eq!(actual_document[key], expected_document[key], "campo {key}");
    }
    assert_eq!(actual_document["patient_id"], "patient-a");
    let expected_text = String::from_utf8_lossy(&original_bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    assert_eq!(actual_document["text"], expected_text);
    assert_eq!(
        actual_document["href"],
        "../documents/duplicate/20260102%20Copia%20sintetica.txt"
    );
    assert_eq!(
        fs::read(&original_path).expect("originale invariato"),
        original_bytes
    );

    let copied = person
        .local_build
        .join("documents/duplicate/20260102 Copia sintetica.txt");
    assert_eq!(fs::read(copied).expect("copia linkata"), original_bytes);
    let html = fs::read_to_string(&result.frontend).expect("pagina HTML");
    assert!(html.contains("<html lang=\"it\">"));
    assert!(html.contains("href=\"../documents/duplicate/20260102%20Copia%20sintetica.txt\""));
    assert!(html.contains("Referto di laboratorio sintetico"));
    assert!(!html.contains(person.source_documents.to_string_lossy().as_ref()));

    let connection = Connection::open(&result.database).expect("database SQLite");
    let count: i64 = connection
        .query_row("SELECT count(*) FROM documents", [], |row| row.get(0))
        .expect("conteggio documenti");
    let indexed: i64 = connection
        .query_row(
            "SELECT count(*) FROM document_fts WHERE document_fts MATCH 'sintetico'",
            [],
            |row| row.get(0),
        )
        .expect("ricerca FTS5");
    assert_eq!(count, 1);
    assert_eq!(indexed, 1);
}

#[test]
fn txt_pipeline_replaces_invalid_utf8_and_escapes_html_and_urls() {
    let directory = TestDirectory::new();
    let source = directory.path().join("source");
    let category = source.join("laboratory");
    fs::create_dir_all(&category).expect("directory documenti");
    let filename = "20260102 <referto>.txt";
    let original = category.join(filename);
    fs::write(&original, [b'<', 0xff, b'\r', b'X']).expect("documento sintetico");
    let person = person(
        directory.path(),
        &source,
        &directory.path().join("metadata"),
    );

    let result = build_txt_pipeline(&person).expect("pipeline con UTF-8 non valido");
    let json: Value =
        serde_json::from_str(&fs::read_to_string(&result.data).expect("JSON esportato"))
            .expect("JSON valido");
    assert_eq!(json["documents"][0]["text"], "<�\nX");
    let html = fs::read_to_string(result.frontend).expect("pagina HTML");
    assert!(html.contains("&lt;referto&gt;"));
    assert!(html.contains("&lt;�\nX"));
    assert!(!html.contains("<referto>"));
    assert!(
        json["documents"][0]["href"]
            .as_str()
            .expect("href relativo")
            .contains("%20%3Creferto%3E.txt")
    );
}

#[test]
fn pdf_pipeline_exports_digital_text_search_and_page_provenance() {
    let directory = TestDirectory::new();
    let source = directory.path().join("source");
    let reports = source.join("reports");
    fs::create_dir_all(&reports).expect("directory documenti sintetici");
    let source_pdf = reports.join("20260102 Synthetic report.pdf");
    let first = "Synthetic non-sensitive PDF text for PyMuPDF integration testing.";
    let second = "Second synthetic page with searchable clinical measurement values.";
    let original = synthetic_pdf(&[first, second]);
    fs::write(&source_pdf, &original).expect("PDF digitale sintetico");
    let person = person(
        directory.path(),
        &source,
        &directory.path().join("metadata"),
    );

    let result = build_pdf_pipeline(&person).expect("pipeline PDF digitale");

    assert_eq!(result.documents, 1);
    assert_eq!(
        fs::read(&source_pdf).expect("originale invariato"),
        original
    );
    let exported: Value =
        serde_json::from_str(&fs::read_to_string(&result.data).expect("JSON PDF"))
            .expect("JSON PDF valido");
    let record = &exported["documents"][0];
    assert!(
        record["text"]
            .as_str()
            .expect("testo estratto")
            .contains(first)
    );
    assert!(
        record["text"]
            .as_str()
            .expect("testo estratto")
            .contains(second)
    );
    assert_eq!(record["warnings"], serde_json::json!([]));
    assert_eq!(
        record["page_spans"]
            .as_array()
            .expect("provenienza pagine")
            .len(),
        2
    );
    assert_eq!(
        record["href"],
        "../documents/reports/20260102%20Synthetic%20report.pdf"
    );
    assert_eq!(
        fs::read(
            person
                .local_build
                .join("documents/reports/20260102 Synthetic report.pdf")
        )
        .expect("PDF linkato"),
        original
    );
    let html = fs::read_to_string(&result.frontend).expect("frontend PDF");
    assert!(html.contains("Archivio PDF"));
    assert!(html.contains("Synthetic non-sensitive PDF text"));

    let connection = Connection::open(&result.database).expect("database SQLite");
    let indexed: i64 = connection
        .query_row(
            "SELECT count(*) FROM document_fts WHERE document_fts MATCH 'measurement'",
            [],
            |row| row.get(0),
        )
        .expect("ricerca FTS sul PDF");
    let page_rows: i64 = connection
        .query_row("SELECT count(*) FROM document_page_spans", [], |row| {
            row.get(0)
        })
        .expect("righe di provenienza pagine");
    let page_two_start: i64 = connection
        .query_row(
            "SELECT character_start FROM document_page_spans WHERE page_number = 2",
            [],
            |row| row.get(0),
        )
        .expect("offset pagina 2");
    assert_eq!(indexed, 1);
    assert_eq!(page_rows, 2);
    assert!(page_two_start > 0);
}

#[test]
fn pdf_pipeline_preserves_extraction_warnings_in_json() {
    let directory = TestDirectory::new();
    let source = directory.path().join("source");
    fs::create_dir_all(&source).expect("directory documenti sintetici");
    fs::write(source.join("20260102 Broken.pdf"), b"not a PDF").expect("PDF malformato sintetico");
    let person = person(
        directory.path(),
        &source,
        &directory.path().join("metadata"),
    );

    let result = build_pdf_pipeline(&person).expect("pipeline con PDF malformato");

    let exported: Value =
        serde_json::from_str(&fs::read_to_string(result.data).expect("JSON con warning"))
            .expect("JSON valido");
    assert!(
        exported["documents"][0]["text"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        exported["documents"][0]["warnings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        exported["documents"][0]["page_spans"],
        serde_json::json!([])
    );
}
