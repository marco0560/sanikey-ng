use super::{extract_pdf_text, pdf_page_count, test_support::synthetic_pdf};
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
fn malformed_and_text_poor_pdfs_return_warnings_without_ocr() {
    let directory = TestDirectory::new();
    let malformed = directory.write_pdf("malformed.pdf", b"not a PDF");
    let malformed_result = extract_pdf_text(&malformed);
    assert!(malformed_result.text.is_empty());
    assert_eq!(malformed_result.page_spans, []);
    assert_eq!(malformed_result.warnings.len(), 1);
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
