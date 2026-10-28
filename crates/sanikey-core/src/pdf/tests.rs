use super::{
    PdfExtractionOptions, bind_pdfium, extract_pdf_text, extract_pdf_text_with_options,
    find_tesseract, pdf_page_count, run_tesseract,
    test_support::{synthetic_pdf, synthetic_scanned_pdf, synthetic_scanned_pdf_with_digital_text},
};
use crate::metadata::DocumentRecord;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sanikey-ng-pdf-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&path).expect("directory PDF sintetica");
        Self(path)
    }

    fn write_pdf(&self, name: &str, bytes: &[u8]) -> DocumentRecord {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("PDF sintetico");
        document(&path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn document(path: &Path) -> DocumentRecord {
    DocumentRecord {
        document_id: "synthetic-pdf-id".to_owned(),
        patient_id: "patient-a".to_owned(),
        path: path.to_path_buf(),
        title: "PDF sintetico".to_owned(),
        category: "reports".to_owned(),
        kind: "pdf".to_owned(),
        sha256: "synthetic-sha256".to_owned(),
        date: None,
        series: None,
        tags: vec![],
        origin: "source".to_owned(),
        container_id: None,
        internal_path: None,
    }
}

#[test]
fn extracts_each_digital_page_and_records_unicode_offsets() {
    let directory = TestDirectory::new();
    let first = "Synthetic non-sensitive PDF text for PyMuPDF integration testing.";
    let second = "Second synthetic page with café and enough searchable content.";
    let record = directory.write_pdf("two-pages.pdf", &synthetic_pdf(&[first, second]));

    let extracted = extract_pdf_text(&record);

    assert_eq!(extracted.document_id, record.document_id);
    assert!(extracted.text.contains(first));
    assert!(
        extracted.text.contains(second),
        "testo estratto: {:?}",
        extracted.text
    );
    assert!(extracted.warnings.is_empty());
    assert_eq!(pdf_page_count(&record), Some(2));
    assert_eq!(extracted.page_spans.len(), 2);
    let spans = &extracted.page_spans;
    assert_eq!(spans[0].page_number, 1);
    assert_eq!(spans[0].character_start, 0);
    assert_eq!(
        spans[0].character_end,
        extracted.text[..extracted.text.find('\n').expect("separatore pagine")]
            .chars()
            .count()
            + 1
    );
    assert_eq!(spans[1].page_number, 2);
    assert_eq!(
        spans[1].character_start,
        spans[0].character_end + 1,
        "offset successivo al separatore newline"
    );
    assert_eq!(spans[1].character_end, extracted.text.chars().count());
}

#[test]
fn malformed_and_text_poor_pdfs_keep_extraction_warnings() {
    let directory = TestDirectory::new();
    let malformed = directory.write_pdf("malformed.pdf", b"not a PDF");
    let malformed_result = extract_pdf_text(&malformed);
    assert!(malformed_result.text.is_empty());
    assert_eq!(malformed_result.page_spans, []);
    assert!(!malformed_result.warnings.is_empty());
    assert!(malformed_result.warnings[0].contains("impossibile leggere"));
    assert_eq!(pdf_page_count(&malformed), None);

    let image_only = directory.write_pdf("image-only.pdf", &synthetic_pdf(&[""]));
    let image_result = extract_pdf_text(&image_only);
    assert!(
        image_result.text.trim().is_empty(),
        "testo estratto: {:?}",
        image_result.text
    );
    assert_eq!(image_result.page_spans.len(), 1);
    assert_eq!(image_result.warnings.len(), 1);
    assert!(image_result.warnings[0].contains("OCR"));
    assert_eq!(pdf_page_count(&image_only), Some(1));
}

#[test]
fn extracts_text_from_a_synthetic_scanned_pdf() {
    let Ok(tesseract) = find_tesseract() else {
        eprintln!("OCR sintetico saltato: PDFium o Tesseract non disponibile");
        return;
    };
    let english_model_available = std::process::Command::new(&tesseract)
        .arg("--list-langs")
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|language| language.trim() == "eng")
        });
    if bind_pdfium().is_err() || !english_model_available {
        eprintln!("OCR sintetico saltato: PDFium o modello Tesseract eng non disponibile");
        return;
    }
    let directory = TestDirectory::new();
    let record = directory.write_pdf(
        "scanned.pdf",
        &synthetic_scanned_pdf("TEST TEST TEST TEST TEST TEST TEST TEST TEST TEST"),
    );

    let extracted = extract_pdf_text(&record);

    assert!(
        extracted.text.matches("TEST").count() >= 8,
        "testo OCR: {:?}; warning: {:?}",
        extracted.text,
        extracted.warnings
    );
    assert!(extracted.warnings.is_empty(), "{:?}", extracted.warnings);
    assert_eq!(extracted.page_spans.len(), 1);
    assert_eq!(extracted.page_spans[0].page_number, 1);

    let digital_record = directory.write_pdf(
        "scanned-with-digital-text.pdf",
        &synthetic_scanned_pdf_with_digital_text(
            "TEST TEST TEST TEST TEST TEST TEST TEST TEST TEST",
            Some("This synthetic digital text is sufficiently long to skip OCR."),
        ),
    );
    let native = extract_pdf_text(&digital_record);
    assert!(native.text.contains("sufficiently long to skip OCR"));
    let forced = extract_pdf_text_with_options(
        &digital_record,
        PdfExtractionOptions {
            force_pdf_ocr: true,
        },
    );
    assert!(
        forced.text.matches("TEST").count() >= 8,
        "testo OCR forzato: {:?}; warning: {:?}",
        forced.text,
        forced.warnings
    );
}

#[test]
fn tesseract_launch_failure_is_reported_without_exposing_stderr() {
    let result = run_tesseract(
        Path::new("/missing/sanikey-test-tesseract"),
        Path::new("/missing/glossary"),
        b"P6\n1 1\n255\n\0\0\0",
        1,
    );

    assert!(
        result
            .unwrap_err()
            .contains("avvio di Tesseract non riuscito")
    );
}

#[test]
fn medical_glossary_is_canonical_and_matches_the_baseline() {
    let glossary = include_str!("../../assets/ocr/tesseract-medical-it.user-words");
    let entries: Vec<&str> = glossary.lines().collect();

    assert!(glossary.ends_with('\n'));
    assert!(!glossary.starts_with('\u{feff}'));
    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .all(|entry| !entry.is_empty() && *entry == entry.trim())
    );
    let mut unique = entries.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), entries.len());
    assert!(entries.contains(&"HbA1c"));
    assert!(!entries.contains(&"HbAlc"));
}
