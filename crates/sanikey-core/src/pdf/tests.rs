//! Fixture sintetiche per estrazione PDF digitale e OCR.

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

/// Gestisce i PDF sintetici e i loro percorsi temporanei.
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

/// Rimuove i PDF sintetici quando i test terminano.
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

#[test]
fn synthetic_trace_captures_pdf_text_and_ocr_boundary_while_events_omit_it() {
    use super::rgba_to_ppm;
    use crate::logging::{Component, Level, Session};

    let directory = TestDirectory::new();
    let pdf_path = {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            directory
                .0
                .join(std::ffi::OsString::from_vec(b"synthetic-\xff.pdf".to_vec()))
        }
        #[cfg(not(unix))]
        {
            directory.0.join("synthetic.pdf")
        }
    };
    fs::write(
        &pdf_path,
        synthetic_pdf(&[
            "Synthetic private PDF text with enough characters to avoid OCR; password = synthetic-pdf-secret; detail retained.",
        ]),
    )
    .expect("PDF sintetico");
    let record = document(&pdf_path);

    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let logs = directory.0.join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        session.scoped(|| {
            let trace =
                crate::logging::trace_current(Component::Core, "pdf::synthetic_ocr_stdout", || {
                    vec![crate::logging::TraceField::new(
                        "stdout",
                        super::trace_pdf_bytes(
                            b"api_key = synthetic-ocr-secret; private OCR output retained",
                        ),
                    )]
                });
            if let Some(trace) = trace {
                let _ = trace.finish(crate::logging::TraceValue::data("completed"));
            }
        });
        let extracted = session.scoped(|| extract_pdf_text(&record));
        assert!(extracted.text.contains("Synthetic private PDF text"));
        assert_eq!(session.scoped(|| pdf_page_count(&record)), Some(1));
        let ppm = session
            .scoped(|| rgba_to_ppm(&[10, 20, 30, 255, 40, 50, 60, 255], 2, 1))
            .expect("PPM sintetico");
        let tesseract = session.scoped(|| {
            run_tesseract(
                Path::new("/missing/sanikey-test-tesseract"),
                Path::new("/missing/sanikey-test-glossary"),
                &ppm,
                1,
            )
        });
        assert!(tesseract.is_err());
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("pdf::extract_pdf_text"));
            assert!(log.contains("pdf::extract_digital_pdf_text"));
            assert!(log.contains("pdf::has_sufficient_pdf_text"));
            assert!(log.contains("pdf::pdf_page_count"));
            assert!(log.contains("pdf_file_bytes"));
            assert!(log.contains("external_component"));
            assert!(log.contains("Synthetic private PDF text"));
            assert!(log.contains("secret_omitted"));
            assert!(log.contains("detail retained"));
            assert!(log.contains("private OCR output retained"));
            assert!(!log.contains("synthetic-pdf-secret"));
            assert!(!log.contains("synthetic-ocr-secret"));
            assert!(log.contains("pdf::rgba_to_ppm"));
            assert!(log.contains("pdf::run_tesseract"));
            assert!(log.contains("stdin_ppm_bytes"));
            assert!(log.contains("stderr_capture"));
            #[cfg(unix)]
            {
                assert!(log.contains("unix_bytes"));
                assert!(log.contains("255"));
            }
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("Synthetic private PDF text"));
            assert!(!log.contains("synthetic-pdf-secret"));
            assert!(!log.contains("synthetic-"));
        }
    }
}
