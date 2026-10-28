//! Comparatori JSON, testo e frontend per gli artefatti di migrazione.

use super::files::{collect_files, compare_file_hash, compare_file_sets};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub(super) fn compare_json_files(reference: &Path, candidate: &Path) -> Result<(), String> {
    let reference_json: Value = serde_json::from_slice(
        &fs::read(reference)
            .map_err(|error| format!("impossibile leggere {}: {error}", reference.display()))?,
    )
    .map_err(|error| format!("JSON non valido in {}: {error}", reference.display()))?;
    let candidate_json: Value = serde_json::from_slice(
        &fs::read(candidate)
            .map_err(|error| format!("impossibile leggere {}: {error}", candidate.display()))?,
    )
    .map_err(|error| format!("JSON non valido in {}: {error}", candidate.display()))?;
    compare_json_values(&reference_json, &candidate_json)
}

pub(super) fn compare_json_values(reference: &Value, candidate: &Value) -> Result<(), String> {
    if reference == candidate {
        Ok(())
    } else {
        Err("struttura o valori JSON differenti".to_owned())
    }
}

pub(super) fn compare_text_files(reference: &Path, candidate: &Path) -> Result<(), String> {
    let reference_text = fs::read_to_string(reference)
        .map_err(|error| format!("testo di riferimento non UTF-8 o illeggibile: {error}"))?;
    let candidate_text = fs::read_to_string(candidate)
        .map_err(|error| format!("testo candidato non UTF-8 o illeggibile: {error}"))?;
    compare_text(&reference_text, &candidate_text)
}

pub(super) fn compare_text(reference: &str, candidate: &str) -> Result<(), String> {
    if normalize_line_endings(reference) == normalize_line_endings(candidate) {
        Ok(())
    } else {
        let line = normalize_line_endings(reference)
            .lines()
            .zip(normalize_line_endings(candidate).lines())
            .position(|(left, right)| left != right)
            .map_or_else(
                || reference.lines().count().min(candidate.lines().count()) + 1,
                |index| index + 1,
            );
        Err(format!("testo differente dalla riga {line}"))
    }
}

fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}

pub(super) fn compare_frontend_directories(
    reference: &Path,
    candidate: &Path,
) -> Result<(), String> {
    if !reference.is_dir() || !candidate.is_dir() {
        return Err("il confronto frontend richiede due directory".to_owned());
    }
    let reference_files = collect_files(reference)?;
    let candidate_files = collect_files(candidate)?;
    compare_file_sets(
        "frontend",
        &reference_files,
        &candidate_files,
        compare_frontend_file,
    )
}

fn compare_frontend_file(reference: &Path, candidate: &Path) -> Result<(), String> {
    let extension = reference
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "json" {
        compare_json_files(reference, candidate)
    } else if matches!(
        extension.as_str(),
        "css" | "csv" | "html" | "js" | "md" | "svg" | "txt" | "xml"
    ) {
        compare_text_files(reference, candidate)
    } else {
        compare_file_hash(reference, candidate)
    }
}
