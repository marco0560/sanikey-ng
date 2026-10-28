//! Account file loading and top-level validation.

use super::super::AccountsConfig;
use super::super::sections::{parse_ingestion, parse_search, parse_ui, parse_usb};
use super::super::validation::{error, reject_unknown, require_table};
use super::paths::{absolute_non_strict, expand_path, path_base};
use super::person::{parse_person, validate_enabled_usb_uuid, validate_unique_ids};
use super::root_section;
use crate::operation::OperationError;
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;

const ROOT_FIELDS: &[&str] = &["global", "person"];
const GLOBAL_FIELDS: &[&str] = &[
    "config_version",
    "repository_version",
    "ui",
    "search",
    "ingestion",
    "usb",
];
/// Restituisce `config/accounts.toml` sotto la directory indicata o quella corrente.
///
/// Se la directory corrente non è disponibile, restituisce un errore di configurazione.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` se non è possibile leggere la directory
/// corrente quando `repo_root` è `None`.
pub fn default_accounts_path(repo_root: Option<&Path>) -> Result<PathBuf, OperationError> {
    let root = match repo_root {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()
            .map_err(|cause| error(format!("directory corrente non disponibile: {cause}")))?,
    };
    Ok(root.join("config").join("accounts.toml"))
}

/// Carica e valida un file TOML di account.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` se il file manca, non è leggibile,
/// contiene TOML non valido o viola il contratto delle sezioni account.
pub fn load_accounts(path: impl AsRef<Path>) -> Result<AccountsConfig, OperationError> {
    let provided = path.as_ref();
    let expanded = expand_path(provided);
    let resolved = match fs::canonicalize(&expanded) {
        Ok(path) if path.is_file() => path,
        _ => {
            let shown = absolute_non_strict(&expanded)?;
            return Err(error(format!(
                "configurazione account non trovata: {}",
                shown.display()
            )));
        }
    };
    let source = fs::read_to_string(&resolved).map_err(|cause| {
        error(format!(
            "impossibile leggere configurazione account {}: {cause}",
            resolved.display()
        ))
    })?;
    let value: Value = toml::from_str(&source).map_err(|cause| {
        error(format!(
            "TOML non valido in {}: {cause}",
            resolved.display()
        ))
    })?;
    parse_accounts_data(&value, &resolved)
}

/// Valida una struttura TOML già analizzata e risolve i percorsi relativi.
///
/// I campi sconosciuti a root, global e sezione persona vengono rifiutati.
/// I percorsi relativi sono ancorati alla root del repository se `path` è sotto
/// una directory chiamata `config`; negli altri casi usano la directory di `path`.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` per strutture, valori, percorsi o
/// precedenze non validi.
pub fn parse_accounts_data(
    data: &Value,
    path: impl AsRef<Path>,
) -> Result<AccountsConfig, OperationError> {
    let config_path = path.as_ref();
    let root = require_table(data, "configurazione")?;
    reject_unknown(root, ROOT_FIELDS, "configurazione")?;

    let global = match root.get("global") {
        Some(value) => require_table(value, "[global]")?,
        None => {
            return Err(error("[global].config_version e' obbligatorio"));
        }
    };
    let version = global
        .get("config_version")
        .or_else(|| global.get("repository_version"))
        .and_then(Value::as_integer)
        .ok_or_else(|| {
            if global.contains_key("config_version") || global.contains_key("repository_version") {
                error("[global].config_version deve essere un intero")
            } else {
                error("[global].config_version e' obbligatorio")
            }
        })?;
    reject_unknown(global, GLOBAL_FIELDS, "[global]")?;

    let people = root
        .get("person")
        .and_then(Value::as_array)
        .filter(|people| !people.is_empty())
        .ok_or_else(|| error("e' richiesta almeno una voce [[person]]"))?;
    let base_dir = path_base(config_path)?;
    let global_ui = parse_ui(root_section(global, "ui")?, "[global.ui]", &base_dir, None)?;
    let global_search = parse_search(
        root_section(global, "search")?,
        "[global.search]",
        &base_dir,
        None,
    )?;
    let global_ingestion = parse_ingestion(
        root_section(global, "ingestion")?,
        "[global.ingestion]",
        None,
    )?;
    let global_usb = parse_usb(root_section(global, "usb")?, "[global.usb]")?;

    let mut parsed_people = Vec::with_capacity(people.len());
    for (index, item) in people.iter().enumerate() {
        parsed_people.push(parse_person(
            item,
            index,
            &base_dir,
            &global_ui,
            &global_search,
            &global_ingestion,
            &global_usb,
        )?);
    }
    validate_unique_ids(&parsed_people)?;
    validate_enabled_usb_uuid(&parsed_people, &global_usb)?;

    Ok(AccountsConfig {
        config_version: version,
        people: parsed_people,
        path: config_path.to_path_buf(),
        ui: global_ui,
        search: global_search,
        ingestion: global_ingestion,
        usb: global_usb,
    })
}
