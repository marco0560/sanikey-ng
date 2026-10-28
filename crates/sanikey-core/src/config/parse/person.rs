//! Per-person account parsing and cross-person validation.

use super::super::sections::{parse_ingestion, parse_search, parse_ui};
use super::super::validation::{
    error, optional_bool, optional_nullable_string, reject_unknown, require_table, required_string,
};
use super::super::{IngestionConfig, PersonConfig, SearchConfig, UiConfig, UsbConfig};
use super::paths::resolve_person_path;
use super::root_section;
use crate::operation::OperationError;
use std::collections::BTreeSet;
use std::path::Path;
use toml::Value;

const PERSON_FIELDS: &[&str] = &[
    "id",
    "display_name",
    "source_documents",
    "metadata_directory",
    "local_build",
    "enabled",
    "ingestion",
    "search",
    "ui",
    "usb_uuid",
];
const REQUIRED_PERSON_FIELDS: &[&str] = &[
    "id",
    "display_name",
    "source_documents",
    "metadata_directory",
    "local_build",
];

pub(super) fn parse_person(
    value: &Value,
    index: usize,
    base_dir: &Path,
    global_ui: &UiConfig,
    global_search: &SearchConfig,
    global_ingestion: &IngestionConfig,
    global_usb: &UsbConfig,
) -> Result<PersonConfig, OperationError> {
    let context = format!("voce [[person]] {index}");
    let table = require_table(value, &context)?;
    reject_unknown(table, PERSON_FIELDS, &context)?;
    let missing: Vec<_> = REQUIRED_PERSON_FIELDS
        .iter()
        .filter(|field| !table.contains_key(**field))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(error(format!(
            "{context} campi mancanti: {}",
            missing.join(", ")
        )));
    }

    let id = required_string(table, "id", &context)?;
    if !valid_person_id(&id) {
        return Err(error(format!(
            "{context} ha id non valido {id:?}; usare lettere minuscole, cifre e trattini"
        )));
    }
    let enabled = optional_bool(table, "enabled", &context, true)?;
    let ui = parse_ui(
        root_section(table, "ui")?,
        &format!("[[person]] entry {index} ui"),
        base_dir,
        Some(global_ui),
    )?;
    let search = parse_search(
        root_section(table, "search")?,
        &format!("[[person]] entry {index} search"),
        base_dir,
        Some(global_search),
    )?;
    let ingestion = parse_ingestion(
        root_section(table, "ingestion")?,
        &format!("[[person]] entry {index} ingestion"),
        Some(global_ingestion),
    )?;
    let usb_context = format!("[[person]] entry {index}");
    let usb_uuid = optional_nullable_string(table, "usb_uuid", &usb_context)?
        .or_else(|| global_usb.usb_uuid.clone())
        .ok_or_else(|| {
            error(format!(
                "{context} campi mancanti: usb_uuid or [global.usb].usb_uuid"
            ))
        })?;

    Ok(PersonConfig {
        id,
        display_name: required_string(table, "display_name", &context)?,
        source_documents: resolve_person_path(table, "source_documents", index, base_dir)?,
        metadata_directory: resolve_person_path(table, "metadata_directory", index, base_dir)?,
        local_build: resolve_person_path(table, "local_build", index, base_dir)?,
        usb_uuid,
        enabled,
        ui,
        search,
        ingestion,
    })
}

fn valid_person_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_lowercase()
        && is_id_letter_or_digit(bytes[bytes.len() - 1])
        && bytes.iter().enumerate().all(|(index, byte)| {
            is_id_letter_or_digit(*byte)
                || (*byte == b'-'
                    && index > 0
                    && index + 1 < bytes.len()
                    && is_id_letter_or_digit(bytes[index - 1])
                    && is_id_letter_or_digit(bytes[index + 1]))
        })
}

fn is_id_letter_or_digit(value: u8) -> bool {
    value.is_ascii_lowercase() || value.is_ascii_digit()
}

pub(super) fn validate_unique_ids(people: &[PersonConfig]) -> Result<(), OperationError> {
    let mut seen = BTreeSet::new();
    for person in people {
        if !seen.insert(&person.id) {
            return Err(error(format!("id paziente duplicato: {}", person.id)));
        }
    }
    Ok(())
}

pub(super) fn validate_enabled_usb_uuid(
    people: &[PersonConfig],
    usb: &UsbConfig,
) -> Result<(), OperationError> {
    let enabled_uuids: BTreeSet<_> = people
        .iter()
        .filter(|person| person.enabled)
        .map(|person| person.usb_uuid.trim())
        .collect();
    if let Some(global_uuid) = &usb.usb_uuid {
        let mismatches: Vec<_> = enabled_uuids
            .into_iter()
            .filter(|uuid| *uuid != global_uuid)
            .collect();
        if !mismatches.is_empty() {
            return Err(error(format!(
                "[global.usb].usb_uuid e' in conflitto con i valori usb_uuid dei pazienti abilitati: {}",
                mismatches.join(", ")
            )));
        }
    } else if enabled_uuids.len() > 1 {
        return Err(error(
            "i pazienti abilitati dichiarano usb_uuid differenti senza [global.usb].usb_uuid",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod internal_tests {
    use super::valid_person_id;

    #[test]
    fn person_id_uses_lowercase_ascii_words_separated_by_hyphens() {
        for accepted in ["a", "a1", "patient-a", "a-1-b"] {
            assert!(valid_person_id(accepted));
        }
        for rejected in ["", "A", "a-", "-a", "a--b", "a_b", "a b", "é"] {
            assert!(!valid_person_id(rejected), "{rejected}");
        }
    }
}
