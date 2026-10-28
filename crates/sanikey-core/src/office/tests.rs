use super::{
    OfficeConverter, OfficeRenderOutcome, converter_for_suffix, find_program, find_program_in_path,
    is_valid_pdf, path_to_file_url, render_office_document, supports_conversion,
};
use crate::metadata::DocumentRecord;
use crate::pdf::test_support::synthetic_pdf;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn office_extensions_select_the_baseline_converter() {
    for extension in ["DOCX", "odt", "rtf"] {
        assert_eq!(
            converter_for_suffix(PathBuf::from(format!("report.{extension}")).as_path()),
            Some(OfficeConverter::Pandoc)
        );
    }
    for extension in ["doc", "ods", "xls", "xlsb", "xlsm", "xlsx"] {
        assert_eq!(
            converter_for_suffix(PathBuf::from(format!("report.{extension}")).as_path()),
            Some(OfficeConverter::LibreOffice)
        );
    }
    for extension in ["html", "pdf", "pptx", "bin"] {
        assert_eq!(
            converter_for_suffix(PathBuf::from(format!("report.{extension}")).as_path()),
            None
        );
    }
}

#[test]
fn only_office_records_with_a_rendering_route_are_supported() {
    let mut record = document_record("report.docx", "office");
    assert!(supports_conversion(&record));

    record.kind = "binary".to_owned();
    assert!(!supports_conversion(&record));

    record.kind = "office".to_owned();
    record.path.set_extension("pptx");
    assert!(!supports_conversion(&record));
}

#[test]
fn program_lookup_uses_only_the_supplied_search_path() {
    let directory = TestDirectory::new();
    let program = directory.path().join("pandoc");
    fs::write(&program, b"synthetic executable").expect("programma sintetico");

    assert_eq!(
        find_program_in_path("pandoc", directory.path().as_os_str()),
        Some(program)
    );
    assert_eq!(
        find_program_in_path("libreoffice", directory.path().as_os_str()),
        None
    );
}

#[test]
fn libreoffice_profile_url_escapes_spaces_and_unicode() {
    assert_eq!(
        path_to_file_url(PathBuf::from("/tmp/profilo è").as_path()),
        "file:///tmp/profilo%20%C3%A8"
    );
}

#[test]
fn conversion_pdf_validation_rejects_corrupt_output() {
    let directory = TestDirectory::new();
    let valid = directory.path().join("valid.pdf");
    let corrupt = directory.path().join("corrupt.pdf");
    fs::write(
        &valid,
        synthetic_pdf(&["Synthetic converted Office content with searchable text."]),
    )
    .expect("PDF sintetico");
    fs::write(&corrupt, b"not a PDF").expect("output corrotto");

    assert!(is_valid_pdf(&valid));
    assert!(!is_valid_pdf(&corrupt));
}

#[test]
#[ignore = "richiede Pandoc, un motore PDF e LibreOffice installati"]
fn installed_converters_render_synthetic_office_text_to_pdf() {
    let Some(pandoc) = find_program("pandoc") else {
        return;
    };
    let directory = TestDirectory::new();
    let rtf = directory.path().join("synthetic.rtf");
    fs::write(
        &rtf,
        r"{\rtf1\ansi\deff0 {\fonttbl{\f0 Arial;}}\f0\fs24 Synthetic office conversion text.}",
    )
    .expect("RTF sintetico valido");

    if find_program("pdflatex").is_some() {
        let pandoc_output = directory
            .path()
            .join("pandoc-rendered")
            .join("synthetic-id");
        let result =
            render_office_document(&rtf, &pandoc_output, &rtf).expect("avvio conversione Pandoc");
        assert_pdf_contains(&result, "Synthetic office conversion text");

        let corrupt = directory.path().join("corrupt.docx");
        let corrupt_bytes = b"not a DOCX archive";
        fs::write(&corrupt, corrupt_bytes).expect("DOCX corrotto sintetico");
        let corrupt_output = directory
            .path()
            .join("corrupt-rendered")
            .join("synthetic-id");
        let result = render_office_document(&corrupt, &corrupt_output, &corrupt)
            .expect("conversione corrotta non fatale");
        assert!(matches!(result, OfficeRenderOutcome::Warning(_)));
        assert_eq!(
            fs::read(corrupt).expect("originale corrotto invariato"),
            corrupt_bytes
        );
        assert!(!corrupt_output.join("document.pdf").exists());
    }

    let libreoffice = find_program("libreoffice").or_else(|| find_program("soffice"));
    if libreoffice.is_some() {
        let docx = directory.path().join("synthetic.docx");
        let generated = Command::new(pandoc)
            .arg(&rtf)
            .arg("--output")
            .arg(&docx)
            .status()
            .expect("avvio Pandoc per fixture DOCX");
        assert!(
            generated.success(),
            "Pandoc deve generare il DOCX sintetico"
        );

        let libreoffice_output = directory
            .path()
            .join("libreoffice-rendered")
            .join("synthetic-id");
        let legacy_suffix = directory.path().join("synthetic.doc");
        let result = render_office_document(&docx, &libreoffice_output, &legacy_suffix)
            .expect("avvio conversione LibreOffice");
        assert_pdf_contains(&result, "Synthetic office conversion text");
    }
}

#[cfg(unix)]
#[test]
fn converter_process_timeout_kills_the_child() {
    use super::{ProcessFailure, wait_for_process};
    use std::time::Duration;

    let mut child = Command::new("sh")
        .args(["-c", "exec sleep 1"])
        .spawn()
        .expect("shell sintetica disponibile su Unix");

    assert!(matches!(
        wait_for_process(&mut child, Duration::from_millis(10)),
        Err(ProcessFailure::Timeout)
    ));
    assert!(child.try_wait().expect("processo terminato").is_some());
}

fn document_record(path: &str, kind: &str) -> DocumentRecord {
    DocumentRecord {
        document_id: "synthetic-id".to_owned(),
        patient_id: "synthetic-person".to_owned(),
        path: PathBuf::from(path),
        title: "Synthetic document".to_owned(),
        category: "documenti".to_owned(),
        kind: kind.to_owned(),
        sha256: "synthetic-hash".to_owned(),
        date: None,
        series: None,
        tags: Vec::new(),
        origin: "source".to_owned(),
        container_id: None,
        internal_path: None,
    }
}

fn assert_pdf_contains(outcome: &OfficeRenderOutcome, expected: &str) {
    let OfficeRenderOutcome::Rendered(path) = outcome else {
        panic!("conversione PDF reale non riuscita: {outcome:?}");
    };
    let document = lopdf::Document::load(path).expect("PDF convertito valido");
    let pages: Vec<u32> = document.get_pages().keys().copied().collect();
    let text = document.extract_text(&pages).expect("testo PDF convertito");
    assert!(
        text.contains(expected),
        "testo convertito assente: {text:?}"
    );
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT_DIRECTORY: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-office-test-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
