//! Conversione dei documenti Office in copie PDF consultabili.

use crate::logging::{
    Component, TraceField, TraceOmission, TraceValue, trace_current, trace_sensitive_text,
};
use crate::metadata::DocumentRecord;
use crate::operation::{ErrorCode, OperationError};
use serde_json::{Value, json};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CONVERSION_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

macro_rules! trace_office_call {
    ($name:literal, [$($field:expr),* $(,)?], $body:expr, $output:expr) => {{
        let trace = trace_current(Component::Core, $name, || vec![$($field),*]);
        let result = run_office_body($body);
        if let Some(trace) = trace {
            let _ = trace.finish($output(&result));
        }
        result
    }};
}

fn run_office_body<T>(body: impl FnOnce() -> T) -> T {
    body()
}

fn trace_office_error(error: &OperationError) -> TraceValue {
    TraceValue::Data(json!({
        "state":"error",
        "code":error.code().as_str(),
        "message":trace_sensitive_text(error.message()).to_json(),
    }))
}

fn trace_path_list(paths: impl IntoIterator<Item = PathBuf>) -> Value {
    Value::Array(
        paths
            .into_iter()
            .map(|path| TraceValue::path(&path).to_json())
            .collect(),
    )
}

/// Esito non fatale della conversione di un originale Office.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OfficeRenderOutcome {
    /// PDF derivato salvato nella directory generata.
    Rendered(PathBuf),
    /// Conversione non disponibile o input/output non valido.
    Warning(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OfficeConverter {
    Pandoc,
    LibreOffice,
}

struct TemporaryWorkspace {
    path: PathBuf,
}

struct GeneratedPdf {
    path: PathBuf,
    keep: bool,
}

/// Crea e rimuove l'area di lavoro isolata per le conversioni Office.
impl TemporaryWorkspace {
    fn create(parent: &Path) -> Result<Self, OperationError> {
        trace_office_call!(
            "office::TemporaryWorkspace::create",
            [TraceField::new("parent", TraceValue::path(parent))],
            || {
                fs::create_dir_all(parent).map_err(|cause| {
                    filesystem_error(
                        "impossibile creare la directory di conversione",
                        parent,
                        cause,
                    )
                })?;
                for attempt in 0..100 {
                    let path =
                        parent.join(format!(".office-work-{}-{attempt}", std::process::id()));
                    match fs::create_dir(&path) {
                        Ok(()) => return Ok(Self { path }),
                        Err(cause) if cause.kind() == ErrorKind::AlreadyExists => continue,
                        Err(cause) => {
                            return Err(filesystem_error(
                                "impossibile creare l'area temporanea di conversione",
                                &path,
                                cause,
                            ));
                        }
                    }
                }
                Err(OperationError::new(
                    ErrorCode::Configuration,
                    "impossibile riservare un'area temporanea di conversione Office",
                ))
            },
            |result: &Result<Self, OperationError>| match result {
                Ok(workspace) => TraceValue::Data(json!({
                    "state":"created",
                    "path":TraceValue::path(&workspace.path).to_json(),
                })),
                Err(error) => trace_office_error(error),
            }
        )
    }
}

/// Pulisce l'area di lavoro dopo la conversione.
impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let result = fs::remove_dir_all(&self.path);
        let trace = trace_current(Component::Core, "office::TemporaryWorkspace::drop", || {
            vec![TraceField::new("path", TraceValue::path(&self.path))]
        });
        if let Some(trace) = trace {
            let output = match &result {
                Ok(()) => TraceValue::Data(json!({"state":"removed"})),
                Err(error) => TraceValue::Data(json!({
                    "state":"failed",
                    "message":trace_sensitive_text(&error.to_string()).to_json(),
                })),
            };
            let _ = trace.finish(output);
        }
    }
}

/// Rimuove il PDF derivato quando non è stato richiesto di conservarlo.
impl Drop for GeneratedPdf {
    fn drop(&mut self) {
        if !self.keep {
            let result = fs::remove_file(&self.path);
            let trace = trace_current(Component::Core, "office::GeneratedPdf::drop", || {
                vec![
                    TraceField::new("path", TraceValue::path(&self.path)),
                    TraceField::new("keep", TraceValue::data(self.keep)),
                ]
            });
            if let Some(trace) = trace {
                let output = match &result {
                    Ok(()) => TraceValue::Data(json!({"state":"removed"})),
                    Err(error) => TraceValue::Data(json!({
                        "state":"failed",
                        "message":trace_sensitive_text(&error.to_string()).to_json(),
                    })),
                };
                let _ = trace.finish(output);
            }
        }
    }
}

/// Indica se l'estensione Office dispone di un convertitore baseline.
pub(crate) fn supports_conversion(record: &DocumentRecord) -> bool {
    trace_office_call!(
        "office::supports_conversion",
        [
            TraceField::new("record_path", TraceValue::path(&record.path)),
            TraceField::new("record_kind", TraceValue::data(record.kind.as_str())),
        ],
        || converter_for_suffix(&record.path).is_some() && record.kind == "office",
        |result: &bool| TraceValue::data(*result)
    )
}

/// Converte un originale Office senza modificarlo e valida il PDF risultante.
pub(crate) fn render_office_document(
    source: &Path,
    output_directory: &Path,
    suffix_source: &Path,
) -> Result<OfficeRenderOutcome, OperationError> {
    trace_office_call!(
        "office::render_office_document",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("output_directory", TraceValue::path(output_directory)),
            TraceField::new("suffix_source", TraceValue::path(suffix_source)),
            TraceField::new(
                "source_document_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        || {
            let Some(converter) = converter_for_suffix(suffix_source) else {
                return Ok(OfficeRenderOutcome::Warning(
                    "formato Office non supportato per la conversione PDF".to_owned(),
                ));
            };
            let program_name = match converter {
                OfficeConverter::Pandoc => "pandoc",
                OfficeConverter::LibreOffice => "libreoffice",
            };
            let Some(program) = find_program(program_name).or_else(|| {
                (converter == OfficeConverter::LibreOffice)
                    .then(|| find_program("soffice"))
                    .flatten()
            }) else {
                let message = match converter {
                    OfficeConverter::Pandoc => "Pandoc non installato; conversione PDF saltata",
                    OfficeConverter::LibreOffice => {
                        "LibreOffice non installato; conversione PDF saltata"
                    }
                };
                return Ok(OfficeRenderOutcome::Warning(message.to_owned()));
            };

            fs::create_dir_all(output_directory).map_err(|cause| {
                filesystem_error(
                    "impossibile creare la directory dei derivati Office",
                    output_directory,
                    cause,
                )
            })?;
            let workspace = TemporaryWorkspace::create(output_directory)?;
            let mut target = GeneratedPdf {
                path: output_directory.join("document.pdf"),
                keep: false,
            };
            let arguments = command_arguments(converter, source, &target.path, &workspace.path);
            match run_process(&program, &arguments) {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    return Ok(OfficeRenderOutcome::Warning(format!(
                        "conversione PDF {} non riuscita (codice {})",
                        converter_name(converter),
                        status
                            .code()
                            .map_or_else(|| "segnale".to_owned(), |code| code.to_string())
                    )));
                }
                Err(ProcessFailure::Launch) => {
                    return Ok(OfficeRenderOutcome::Warning(format!(
                        "impossibile avviare {}; conversione PDF saltata",
                        converter_name(converter)
                    )));
                }
                Err(ProcessFailure::Timeout) => {
                    return Ok(OfficeRenderOutcome::Warning(format!(
                        "conversione PDF {} interrotta dopo 120 secondi",
                        converter_name(converter)
                    )));
                }
                Err(ProcessFailure::Wait) => {
                    return Ok(OfficeRenderOutcome::Warning(format!(
                        "errore durante l'attesa della conversione PDF {}",
                        converter_name(converter)
                    )));
                }
            }

            let generated = match converter {
                OfficeConverter::Pandoc => target.path.clone(),
                OfficeConverter::LibreOffice => {
                    let Some(stem) = source.file_stem() else {
                        return Ok(OfficeRenderOutcome::Warning(
                            "il documento Office non ha un nome convertibile".to_owned(),
                        ));
                    };
                    workspace.path.join(stem).with_extension("pdf")
                }
            };
            if !is_valid_pdf(&generated) {
                return Ok(OfficeRenderOutcome::Warning(format!(
                    "conversione PDF {} non ha prodotto un PDF valido",
                    converter_name(converter)
                )));
            }

            if converter == OfficeConverter::LibreOffice {
                copy_pdf(&generated, &target.path)?;
            }
            target.keep = true;
            Ok(OfficeRenderOutcome::Rendered(target.path.clone()))
        },
        |result: &Result<OfficeRenderOutcome, OperationError>| match result {
            Ok(OfficeRenderOutcome::Rendered(path)) => TraceValue::Data(json!({
                "outcome":"rendered",
                "path":TraceValue::path(path).to_json(),
            })),
            Ok(OfficeRenderOutcome::Warning(warning)) => TraceValue::Data(json!({
                "outcome":"warning",
                "warning":trace_sensitive_text(warning).to_json(),
            })),
            Err(error) => trace_office_error(error),
        }
    )
}

fn converter_for_suffix(path: &Path) -> Option<OfficeConverter> {
    trace_office_call!(
        "office::converter_for_suffix",
        [TraceField::new("path", TraceValue::path(path))],
        || {
            let suffix = path.extension()?.to_string_lossy();
            if ["docx", "odt", "rtf"]
                .iter()
                .any(|value| suffix.eq_ignore_ascii_case(value))
            {
                Some(OfficeConverter::Pandoc)
            } else if ["doc", "ods", "xls", "xlsb", "xlsm", "xlsx"]
                .iter()
                .any(|value| suffix.eq_ignore_ascii_case(value))
            {
                Some(OfficeConverter::LibreOffice)
            } else {
                None
            }
        },
        |result: &Option<OfficeConverter>| TraceValue::data(result.map(converter_name))
    )
}

fn command_arguments(
    converter: OfficeConverter,
    source: &Path,
    target: &Path,
    workspace: &Path,
) -> Vec<OsString> {
    trace_office_call!(
        "office::command_arguments",
        [
            TraceField::new("converter", TraceValue::data(converter_name(converter))),
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new("workspace", TraceValue::path(workspace)),
        ],
        || match converter {
            OfficeConverter::Pandoc => vec![
                source.as_os_str().to_owned(),
                OsString::from("--output"),
                target.as_os_str().to_owned(),
            ],
            OfficeConverter::LibreOffice => vec![
                OsString::from(format!(
                    "-env:UserInstallation={}",
                    path_to_file_url(&workspace.join("profile"))
                )),
                OsString::from("--headless"),
                OsString::from("--convert-to"),
                OsString::from("pdf"),
                OsString::from("--outdir"),
                workspace.as_os_str().to_owned(),
                source.as_os_str().to_owned(),
            ],
        },
        |result: &Vec<OsString>| TraceValue::Data(json!({
            "arguments_native":result
                .iter()
                .map(|argument| TraceValue::path(Path::new(argument)).to_json())
                .collect::<Vec<_>>(),
        }))
    )
}

fn path_to_file_url(path: &Path) -> String {
    trace_office_call!(
        "office::path_to_file_url",
        [TraceField::new("path", TraceValue::path(path))],
        || {
            let path = path.to_string_lossy().replace('\\', "/");
            let path = if path.starts_with('/') {
                path
            } else {
                format!("/{path}")
            };
            let mut url = String::from("file://");
            for byte in path.bytes() {
                if byte.is_ascii_alphanumeric()
                    || matches!(byte, b'/' | b':' | b'-' | b'.' | b'_' | b'~')
                {
                    url.push(char::from(byte));
                } else {
                    url.push_str(&format!("%{byte:02X}"));
                }
            }
            url
        },
        |result: &String| trace_sensitive_text(result)
    )
}

fn converter_name(converter: OfficeConverter) -> &'static str {
    trace_office_call!(
        "office::converter_name",
        [TraceField::new(
            "converter",
            TraceValue::data(format!("{converter:?}"))
        )],
        || match converter {
            OfficeConverter::Pandoc => "Pandoc",
            OfficeConverter::LibreOffice => "LibreOffice",
        },
        |result: &&str| trace_sensitive_text(result)
    )
}

fn find_program(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH");
    trace_office_call!(
        "office::find_program",
        [
            TraceField::new("name", trace_sensitive_text(name)),
            TraceField::new(
                "path_search_directories",
                path.as_deref().map_or_else(
                    || TraceValue::data(Value::Null),
                    |path| TraceValue::Data(trace_path_list(env::split_paths(path))),
                ),
            ),
        ],
        || path
            .as_deref()
            .and_then(|path| find_program_in_path(name, path)),
        |result: &Option<PathBuf>| result.as_ref().map_or_else(
            || TraceValue::data(Value::Null),
            |path| TraceValue::path(path),
        )
    )
}

fn find_program_in_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    trace_office_call!(
        "office::find_program_in_path",
        [
            TraceField::new("name", trace_sensitive_text(name)),
            TraceField::new(
                "path_search_directories",
                TraceValue::Data(trace_path_list(env::split_paths(path))),
            ),
        ],
        || {
            env::split_paths(path).find_map(|directory| {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
                #[cfg(windows)]
                for extension in ["exe", "cmd", "bat"] {
                    let candidate = directory.join(format!("{name}.{extension}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
                None
            })
        },
        |result: &Option<PathBuf>| result.as_ref().map_or_else(
            || TraceValue::data(Value::Null),
            |path| TraceValue::path(path),
        )
    )
}

#[derive(Debug)]
enum ProcessFailure {
    Launch,
    Timeout,
    Wait,
}

fn trace_process_result(result: &Result<ExitStatus, ProcessFailure>) -> TraceValue {
    match result {
        Ok(status) => TraceValue::Data(json!({
            "state":"exited",
            "success":status.success(),
            "exit_code":status.code(),
        })),
        Err(ProcessFailure::Launch) => TraceValue::Data(json!({"state":"launch_failed"})),
        Err(ProcessFailure::Timeout) => TraceValue::Data(json!({"state":"timed_out"})),
        Err(ProcessFailure::Wait) => TraceValue::Data(json!({"state":"wait_failed"})),
    }
}

fn run_process(program: &Path, arguments: &[OsString]) -> Result<ExitStatus, ProcessFailure> {
    trace_office_call!(
        "office::run_process",
        [
            TraceField::new("program", TraceValue::path(program)),
            TraceField::new(
                "arguments_native",
                TraceValue::Data(json!(
                    arguments
                        .iter()
                        .map(|argument| TraceValue::path(Path::new(argument)).to_json())
                        .collect::<Vec<_>>()
                )),
            ),
            TraceField::new("stdin", TraceValue::data("null")),
            TraceField::new("stdout", TraceValue::data("null")),
            TraceField::new("stderr", TraceValue::data("null")),
            TraceField::new(
                "process_runtime_internals",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        || run_process_impl(program, arguments),
        |result: &Result<ExitStatus, ProcessFailure>| trace_process_result(result)
    )
}

fn run_process_impl(program: &Path, arguments: &[OsString]) -> Result<ExitStatus, ProcessFailure> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ProcessFailure::Launch)?;
    wait_for_process(&mut child, CONVERSION_TIMEOUT)
}

fn wait_for_process(child: &mut Child, timeout: Duration) -> Result<ExitStatus, ProcessFailure> {
    trace_office_call!(
        "office::wait_for_process",
        [
            TraceField::new("pid", TraceValue::data(child.id()),),
            TraceField::new(
                "timeout_millis",
                TraceValue::data(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)),
            ),
            TraceField::new(
                "child_handle",
                TraceValue::Omitted(TraceOmission::UnsupportedType),
            ),
        ],
        || {
            let start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => return Ok(status),
                    Ok(None) if start.elapsed() < timeout => thread::sleep(POLL_INTERVAL),
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(ProcessFailure::Timeout);
                    }
                    Err(_) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(ProcessFailure::Wait);
                    }
                }
            }
        },
        |result: &Result<ExitStatus, ProcessFailure>| trace_process_result(result)
    )
}

fn is_valid_pdf(path: &Path) -> bool {
    trace_office_call!(
        "office::is_valid_pdf",
        [
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new(
                "pdf_parser_internals",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
            TraceField::new(
                "document_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        || {
            let Ok(mut file) = File::open(path) else {
                return false;
            };
            let mut header = [0; 5];
            if file.read_exact(&mut header).is_err() || &header != b"%PDF-" {
                return false;
            }
            lopdf::Document::load(path).is_ok_and(|document| !document.get_pages().is_empty())
        },
        |result: &bool| TraceValue::data(*result)
    )
}

fn copy_pdf(source: &Path, target: &Path) -> Result<u64, OperationError> {
    trace_office_call!(
        "office::copy_pdf",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "copied_pdf_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        || fs::copy(source, target).map_err(|cause| {
            filesystem_error("impossibile salvare il PDF derivato", target, cause)
        }),
        |result: &Result<u64, OperationError>| match result {
            Ok(bytes_copied) => TraceValue::Data(json!({
                "state":"copied",
                "bytes_copied":bytes_copied,
            })),
            Err(error) => trace_office_error(error),
        }
    )
}

fn filesystem_error(action: &str, path: &Path, cause: std::io::Error) -> OperationError {
    trace_office_call!(
        "office::filesystem_error",
        [
            TraceField::new("action", trace_sensitive_text(action)),
            TraceField::new("path", TraceValue::path(path)),
        ],
        || {
            OperationError::new(
                ErrorCode::Configuration,
                format!("{action} {}: {cause}", path.display()),
            )
        },
        |result: &OperationError| trace_office_error(result)
    )
}

#[cfg(test)]
mod tests;
