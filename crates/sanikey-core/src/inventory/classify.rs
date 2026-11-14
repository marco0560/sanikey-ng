//! Classificazione e titoli ricavati dai percorsi dei documenti.

use crate::logging::{Component, TraceField, TraceValue, trace_current};
use serde_json::json;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const DICOM_EXTENSIONS: &[(&str, &str)] = &[
    (".dcm", "dicom_file"),
    (".img", "dicom_img"),
    (".iso", "dicom_iso"),
];
const TEXT_EXTENSIONS: &[&str] = &[".txt", ".md"];
const IMAGE_EXTENSIONS: &[&str] = &[".jpeg", ".jpg", ".png"];
const ARCHIVE_EXTENSIONS: &[&str] = &[".7z", ".rar", ".tar.xz", ".zip"];
const OFFICE_EXTENSIONS: &[&str] = &[".doc", ".docx", ".htm", ".html", ".odt", ".rtf"];
const SPREADSHEET_EXTENSIONS: &[&str] = &[".ods", ".xls", ".xlsb", ".xlsm", ".xlsx"];

/// Determina la categoria di visualizzazione dalla prima directory relativa.
pub(super) fn category(relative: &Path) -> String {
    let trace = trace_current(Component::Core, "inventory::classify::category", || {
        vec![TraceField::new("relative", TraceValue::path(relative))]
    });
    let result = (|| {
        let mut components = relative.components();
        let Some(first) = components.next() else {
            return "uncategorized".to_owned();
        };
        if components.next().is_none() {
            return "uncategorized".to_owned();
        }
        let category = first.as_os_str().to_string_lossy();
        let category = category.trim_start_matches('_');
        if category.is_empty() {
            "uncategorized".to_owned()
        } else {
            category.to_owned()
        }
    })();
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(result)));
    }
    result
}

/// Estrae data e titolo dal nome del file.
pub(super) fn title_from_path(path: &Path) -> (Option<String>, String) {
    let trace = trace_current(
        Component::Core,
        "inventory::classify::title_from_path",
        || vec![TraceField::new("path", TraceValue::path(path))],
    );
    let result = (|| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let stem = title_stem(path, &name);
        let characters: Vec<char> = stem.chars().collect();
        if characters.len() >= 10 && characters[..8].iter().all(char::is_ascii_digit) {
            let mut separator_end = 8;
            while separator_end < characters.len()
                && (characters[separator_end].is_whitespace()
                    || characters[separator_end] == '_'
                    || characters[separator_end] == '-')
            {
                separator_end += 1;
            }
            if separator_end > 8 && separator_end < characters.len() {
                let date = format!(
                    "{}-{}-{}",
                    characters[..4].iter().collect::<String>(),
                    characters[4..6].iter().collect::<String>(),
                    characters[6..8].iter().collect::<String>()
                );
                let title = characters[separator_end..]
                    .iter()
                    .collect::<String>()
                    .replace('_', " ")
                    .trim()
                    .to_owned();
                return (Some(date), title);
            }
        }
        (None, stem.replace('_', " ").trim().to_owned())
    })();
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({
            "date": result.0,
            "title": result.1,
        })));
    }
    result
}

/// Determina il tipo documentale con precedenza per il magic DICOM.
pub(super) fn kind(path: &Path) -> String {
    let trace = trace_current(Component::Core, "inventory::classify::kind", || {
        vec![TraceField::new("path", TraceValue::path(path))]
    });
    let result = (|| {
        if has_dicom_magic(path) {
            return "dicom_file".to_owned();
        }
        let suffix = known_suffix(path);
        if let Some((_, kind)) = DICOM_EXTENSIONS
            .iter()
            .find(|(extension, _)| *extension == suffix)
        {
            return (*kind).to_owned();
        }
        if suffix == ".pdf" {
            return "pdf".to_owned();
        }
        if TEXT_EXTENSIONS.contains(&suffix.as_str()) {
            return "text".to_owned();
        }
        if IMAGE_EXTENSIONS.contains(&suffix.as_str()) {
            return "image".to_owned();
        }
        if ARCHIVE_EXTENSIONS.contains(&suffix.as_str()) {
            return "archive".to_owned();
        }
        if OFFICE_EXTENSIONS.contains(&suffix.as_str())
            || SPREADSHEET_EXTENSIONS.contains(&suffix.as_str())
        {
            return "office".to_owned();
        }
        "binary".to_owned()
    })();
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(result)));
    }
    result
}

fn title_stem(path: &Path, name: &str) -> String {
    let trace = trace_current(Component::Core, "inventory::classify::title_stem", || {
        vec![
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("name", TraceValue::Data(json!(name))),
        ]
    });
    let result = if name.to_lowercase().ends_with(".tar.xz") {
        name[..name.len() - ".tar.xz".len()].to_owned()
    } else {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(result)));
    }
    result
}

fn known_suffix(path: &Path) -> String {
    let trace = trace_current(Component::Core, "inventory::classify::known_suffix", || {
        vec![TraceField::new("path", TraceValue::path(path))]
    });
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let result = if name.to_lowercase().ends_with(".tar.xz") {
        ".tar.xz".to_owned()
    } else {
        path.extension()
            .map(|extension| format!(".{}", extension.to_string_lossy().to_lowercase()))
            .unwrap_or_default()
    };
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(result)));
    }
    result
}

fn has_dicom_magic(path: &Path) -> bool {
    let trace = trace_current(
        Component::Core,
        "inventory::classify::has_dicom_magic",
        || vec![TraceField::new("path", TraceValue::path(path))],
    );
    let mut read_state = "open_failed";
    let mut observed_marker = None;
    let result = (|| {
        let Ok(mut file) = File::open(path) else {
            return false;
        };
        read_state = "seek_failed";
        if file.seek(SeekFrom::Start(128)).is_err() {
            return false;
        }
        read_state = "read_failed";
        let mut marker = [0_u8; 4];
        if file.read_exact(&mut marker).is_err() {
            return false;
        }
        read_state = "read";
        observed_marker = Some(marker);
        marker == *b"DICM"
    })();
    if let Some(trace) = trace {
        let marker = observed_marker.map_or_else(
            || json!({"kind": "omitted", "reason": "external_component"}),
            |marker| json!({"kind": "data", "value": marker}),
        );
        let _ = trace.finish(TraceValue::Data(json!({
            "result": result,
            "read_state": read_state,
            "marker_bytes": marker,
        })));
    }
    result
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::logging::{Component, Level, Session};

    use super::{category, kind, title_from_path};

    struct TestDirectory(PathBuf);

    /// Crea il contenitore temporaneo delle fixture di classificazione.
    impl TestDirectory {
        fn new() -> Self {
            static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "sanikey-classify-trace-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("directory temporanea sintetica");
            Self(path)
        }
    }

    /// Pulisce le fixture temporanee al termine del test.
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn classification_trace_records_private_inputs_and_results_only_at_trace_level() {
        for (level, expect_trace) in [(Level::Events, false), (Level::Trace, true)] {
            let directory = TestDirectory::new();
            let dicom_path = directory.0.join("private-dicom-marker.dcm");
            let dicom_bytes = [vec![0; 128], b"DICM".to_vec(), b"synthetic".to_vec()].concat();
            fs::write(&dicom_path, dicom_bytes).expect("DICOM sintetico");
            let text_path = directory.0.join("private-suffix-marker.txt");
            fs::write(&text_path, b"synthetic text").expect("testo sintetico");
            let (session, log_path) = Session::create_in_directory_with_level(
                &directory.0.join("logs"),
                Component::Core,
                level,
            )
            .expect("sessione sintetica");

            session.scoped(|| {
                assert_eq!(
                    category(Path::new("private-category/20260102 Private_Title.txt")),
                    "private-category"
                );
                assert_eq!(
                    title_from_path(Path::new("private-category/20260102 Private_Title.txt")),
                    (Some("2026-01-02".to_owned()), "Private Title".to_owned())
                );
                assert_eq!(kind(&dicom_path), "dicom_file");
                assert_eq!(kind(&text_path), "text");
            });

            let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
            if expect_trace {
                assert!(log.contains("inventory::classify::category"));
                assert!(log.contains("inventory::classify::title_from_path"));
                assert!(log.contains("inventory::classify::kind"));
                assert!(log.contains("inventory::classify::has_dicom_magic"));
                assert!(log.contains("inventory::classify::known_suffix"));
                assert!(log.contains("private-category"));
                assert!(log.contains("Private_Title"));
                assert!(log.contains("Private Title"));
                assert!(log.contains("private-suffix-marker.txt"));
                assert!(log.contains("dicom_file"));
                assert!(log.contains("\"value\":[68,73,67,77]"));
            } else {
                assert!(!log.contains("inventory::classify"));
                assert!(!log.contains("private-category"));
                assert!(!log.contains("Private_Title"));
                assert!(!log.contains("private-suffix-marker.txt"));
                assert!(!log.contains("DICM"));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn classification_trace_encodes_non_utf8_paths_as_unix_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let directory = TestDirectory::new();
        let relative = PathBuf::from(OsString::from_vec(vec![b'_', 0xff, b'/', b'x']));
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0.join("logs"),
            Component::Core,
            Level::Trace,
        )
        .expect("sessione trace sintetica");

        let value = session.scoped(|| category(&relative));

        assert_eq!(value, "\u{fffd}");
        let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
        assert!(log.contains("unix_bytes"));
        assert!(log.contains("255"));
    }
}
