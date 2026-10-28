//! Classificazione e titoli ricavati dai percorsi dei documenti.

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
}

/// Estrae data e titolo dal nome del file.
pub(super) fn title_from_path(path: &Path) -> (Option<String>, String) {
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
}

/// Determina il tipo documentale con precedenza per il magic DICOM.
pub(super) fn kind(path: &Path) -> String {
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
}

fn title_stem(path: &Path, name: &str) -> String {
    if name.to_lowercase().ends_with(".tar.xz") {
        return name[..name.len() - ".tar.xz".len()].to_owned();
    }
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn known_suffix(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if name.to_lowercase().ends_with(".tar.xz") {
        return ".tar.xz".to_owned();
    }
    path.extension()
        .map(|extension| format!(".{}", extension.to_string_lossy().to_lowercase()))
        .unwrap_or_default()
}

fn has_dicom_magic(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    if file.seek(SeekFrom::Start(128)).is_err() {
        return false;
    }
    let mut marker = [0_u8; 4];
    file.read_exact(&mut marker).is_ok() && marker == *b"DICM"
}
