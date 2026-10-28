//! Conversione dei documenti Office in copie PDF consultabili.

use crate::metadata::DocumentRecord;
use crate::operation::{ErrorCode, OperationError};
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

impl TemporaryWorkspace {
    fn create(parent: &Path) -> Result<Self, OperationError> {
        fs::create_dir_all(parent).map_err(|cause| {
            filesystem_error(
                "impossibile creare la directory di conversione",
                parent,
                cause,
            )
        })?;
        for attempt in 0..100 {
            let path = parent.join(format!(".office-work-{}-{attempt}", std::process::id()));
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
    }
}

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl Drop for GeneratedPdf {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Indica se l'estensione Office dispone di un convertitore baseline.
pub(crate) fn supports_conversion(record: &DocumentRecord) -> bool {
    converter_for_suffix(&record.path).is_some() && record.kind == "office"
}

/// Converte un originale Office senza modificarlo e valida il PDF risultante.
pub(crate) fn render_office_document(
    source: &Path,
    output_directory: &Path,
    suffix_source: &Path,
) -> Result<OfficeRenderOutcome, OperationError> {
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
            OfficeConverter::LibreOffice => "LibreOffice non installato; conversione PDF saltata",
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
        fs::copy(&generated, &target.path).map_err(|cause| {
            filesystem_error("impossibile salvare il PDF derivato", &target.path, cause)
        })?;
    }
    target.keep = true;
    Ok(OfficeRenderOutcome::Rendered(target.path.clone()))
}

fn converter_for_suffix(path: &Path) -> Option<OfficeConverter> {
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
}

fn command_arguments(
    converter: OfficeConverter,
    source: &Path,
    target: &Path,
    workspace: &Path,
) -> Vec<OsString> {
    match converter {
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
    }
}

fn path_to_file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    let mut url = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'.' | b'_' | b'~') {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

fn converter_name(converter: OfficeConverter) -> &'static str {
    match converter {
        OfficeConverter::Pandoc => "Pandoc",
        OfficeConverter::LibreOffice => "LibreOffice",
    }
}

fn find_program(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    find_program_in_path(name, &path)
}

fn find_program_in_path(name: &str, path: &OsStr) -> Option<PathBuf> {
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
}

enum ProcessFailure {
    Launch,
    Timeout,
    Wait,
}

fn run_process(program: &Path, arguments: &[OsString]) -> Result<ExitStatus, ProcessFailure> {
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
}

fn is_valid_pdf(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut header = [0; 5];
    if file.read_exact(&mut header).is_err() || &header != b"%PDF-" {
        return false;
    }
    lopdf::Document::load(path).is_ok_and(|document| !document.get_pages().is_empty())
}

fn filesystem_error(action: &str, path: &Path, cause: std::io::Error) -> OperationError {
    OperationError::new(
        ErrorCode::Configuration,
        format!("{action} {}: {cause}", path.display()),
    )
}

#[cfg(test)]
mod tests;
