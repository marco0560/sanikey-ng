//! Fixture sintetiche per conversioni e consultazione Office.

use super::{
    OfficeConverter, OfficeRenderOutcome, converter_for_suffix, find_program, find_program_in_path,
    is_valid_pdf, path_to_file_url, render_office_document, supports_conversion,
};
use crate::logging::{Component, Level, Session};
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

#[cfg(unix)]
#[test]
fn process_trace_captures_native_command_and_exit_but_events_stay_terse() {
    use super::run_process;
    use serde_json::Value;
    use std::ffi::OsString;
    use std::os::unix::process::ExitStatusExt;

    let directory = TestDirectory::new();
    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let logs = directory.path().join(format!("logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        session.scoped(|| {
            for (command, expected_code, expected_signal) in [
                ("exit 0", Some(0), None),
                ("exit 7", Some(7), None),
                ("kill -TERM $$", None, Some(15)),
            ] {
                let status = run_process(
                    PathBuf::from("/bin/sh").as_path(),
                    &[OsString::from("-c"), OsString::from(command)],
                )
                .expect("processo sintetico");
                assert_eq!(status.code(), expected_code);
                assert_eq!(status.signal(), expected_signal);
            }
        });
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("office::run_process"));
            assert!(log.contains("office::wait_for_process"));
            assert!(log.contains("/bin/sh"));
            assert!(log.contains("arguments_native"));
            assert!(log.contains("exit_code"));
            assert!(log.contains("external_component"));
            let records: Vec<Value> = log
                .lines()
                .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
                .collect();
            let entries: Vec<_> = records
                .iter()
                .filter(|record| {
                    record["type"] == "function_enter"
                        && record["function"] == "office::run_process"
                })
                .collect();
            assert_eq!(entries.len(), 3);
            for (entry, (expected_code, expected_signal)) in entries.into_iter().zip([
                (serde_json::json!(0), Value::Null),
                (serde_json::json!(7), Value::Null),
                (Value::Null, serde_json::json!(15)),
            ]) {
                let call_id = entry["call_id"].as_u64().expect("call_id numerico");
                let exit = records
                    .iter()
                    .find(|record| {
                        record["type"] == "function_exit"
                            && record["function"] == "office::run_process"
                            && record["call_id"] == call_id
                    })
                    .expect("uscita correlata tramite call_id");
                assert!(exit["duration_ms"].as_u64().is_some());
                let result = &exit["result"]["value"];
                assert_eq!(result["exit_code"], expected_code);
                assert_eq!(result["signal"], expected_signal);
            }
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("/bin/sh"));
            assert!(!log.contains("signal"));
        }
    }
}

#[cfg(unix)]
#[test]
fn office_helper_traces_record_arguments_results_durations_and_secret_omissions() {
    use super::{find_program_in_path, path_to_file_url, supports_conversion};
    use serde_json::json;

    fn path_value(path: &std::path::Path) -> serde_json::Value {
        json!({
            "kind": "data",
            "value": {
                "encoding": "utf8",
                "value": path.to_str().expect("path sintetico UTF-8"),
            },
        })
    }

    fn assert_call(
        records: &[serde_json::Value],
        function: &str,
        arguments: serde_json::Value,
        result: serde_json::Value,
    ) {
        let entries: Vec<_> = records
            .iter()
            .filter(|record| record["type"] == "function_enter" && record["function"] == function)
            .collect();
        assert_eq!(entries.len(), 1, "ingresso univoco di {function}");
        assert!(entries[0]["call_id"].as_u64().is_some());
        assert_eq!(
            entries[0]["arguments"], arguments,
            "argomenti di {function}"
        );

        let exits: Vec<_> = records
            .iter()
            .filter(|record| {
                record["type"] == "function_exit"
                    && record["function"] == function
                    && record["call_id"] == entries[0]["call_id"]
            })
            .collect();
        assert_eq!(exits.len(), 1, "uscita correlata di {function}");
        assert_eq!(exits[0]["outcome"], "returned");
        assert!(exits[0]["duration_ms"].as_u64().is_some());
        assert_eq!(exits[0]["result"], result, "risultato di {function}");
    }

    let directory = TestDirectory::new();
    let executable = directory.path().join("pandoc");
    fs::write(&executable, b"synthetic executable").expect("eseguibile sintetico");
    let lookup_path = directory.path().as_os_str();
    let ordinary_path = PathBuf::from("/synthetic/profilo è");
    let secret_source = directory
        .path()
        .join("api_key=SYNTHETIC-OFFICE-TRACE-SECRET/source.docx");
    let record = document_record(
        secret_source.to_str().expect("path sintetico UTF-8"),
        "office",
    );

    let logs = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");
    let (found, url, supported) = session.scoped(|| {
        (
            find_program_in_path("pandoc", lookup_path),
            path_to_file_url(&ordinary_path),
            supports_conversion(&record),
        )
    });
    assert_eq!(found, Some(executable.clone()));
    assert_eq!(url, "file:///synthetic/profilo%20%C3%A8");
    assert!(supported);
    session.finish(Component::Core).expect("chiusura Trace");

    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(!log.contains("SYNTHETIC-OFFICE-TRACE-SECRET"));
    assert!(log.contains("secret_omitted"));
    let records: Vec<serde_json::Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
        .collect();
    let expected_directory = path_value(directory.path());
    assert_call(
        &records,
        "office::find_program_in_path",
        json!([
            {
                "name": "name",
                "content": {
                    "kind": "data",
                    "value": {
                        "format": "text_redacted_known_secret_assignments",
                        "value": "pandoc",
                    },
                },
            },
            {
                "name": "path_search_directories",
                "content": {"kind": "data", "value": [expected_directory]},
            },
        ]),
        path_value(&executable),
    );
    assert_call(
        &records,
        "office::path_to_file_url",
        json!([{"name": "path", "content": path_value(&ordinary_path)}]),
        json!({
            "kind": "data",
            "value": {
                "format": "text_redacted_known_secret_assignments",
                "value": "file:///synthetic/profilo%20%C3%A8",
            },
        }),
    );
    assert_call(
        &records,
        "office::supports_conversion",
        json!([
            {"name": "record_path", "content": {"kind": "secret_omitted"}},
            {"name": "record_kind", "content": {"kind": "data", "value": "office"}},
        ]),
        json!({"kind": "data", "value": true}),
    );
}

#[test]
fn render_trace_marks_source_contents_as_unobserved_for_unsupported_formats() {
    let directory = TestDirectory::new();
    let source = directory.path().join("Synthetic Private Office.pptx");
    let output = directory.path().join("generated");
    let logs = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione sintetica");
    let outcome = session
        .scoped(|| render_office_document(&source, &output, &source).expect("warning non fatale"));
    assert!(matches!(outcome, OfficeRenderOutcome::Warning(_)));
    drop(session);

    let log = fs::read_to_string(log_path).expect("log sintetico");
    assert!(log.contains("office::render_office_document"));
    assert!(log.contains("source_document_contents"));
    assert!(log.contains("external_component"));
    assert!(log.contains("Synthetic Private Office.pptx"));
    assert!(!source.exists());
}

#[test]
fn pdf_copy_trace_records_reported_byte_count_without_reading_payload() {
    use super::copy_pdf;

    let directory = TestDirectory::new();
    let source = directory.path().join("Synthetic private PDF source.pdf");
    fs::write(&source, b"%PDF-synthetic-private-payload").expect("PDF sintetico");
    let payload_length = fs::metadata(&source).expect("metadata sorgente").len();

    for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
        let logs = directory.path().join(format!("copy-logs-{suffix}"));
        let (session, log_path) =
            Session::create_in_directory_with_level(&logs, Component::Core, level)
                .expect("sessione sintetica");
        let target = directory.path().join(format!("target-{suffix}.pdf"));
        let copied = session.scoped(|| copy_pdf(&source, &target).expect("copia sintetica"));
        assert_eq!(copied, payload_length);
        assert_eq!(
            fs::read(&target).expect("PDF copiato"),
            b"%PDF-synthetic-private-payload"
        );
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico");
        if level == Level::Trace {
            assert!(log.contains("office::copy_pdf"));
            assert!(log.contains("copied_pdf_contents"));
            assert!(log.contains("bytes_copied"));
            assert!(log.contains("external_component"));
            assert!(!log.contains("synthetic-private-payload"));
        } else {
            assert!(!log.contains("function_enter"));
            assert!(!log.contains("synthetic-private-payload"));
        }
    }
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

/// Gestisce file di input e output temporanei per i test Office.
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

/// Elimina gli artefatti Office generati dalle fixture.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
