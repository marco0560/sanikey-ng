//! Search section and ordered dictionary parsing.

use super::super::validation::{error, optional_positive_integer, reject_unknown, table_or_empty};
use super::super::{SearchConfig, SearchDictionary};
use super::paths::optional_path;
use crate::operation::OperationError;
use indexmap::IndexMap;
use std::fs;
use std::path::Path;
use toml::Value;

const SEARCH_FIELDS: &[&str] = &["advanced_index_warning_mb", "dictionary"];
pub(in crate::config) fn parse_search(
    value: &Value,
    context: &str,
    base_dir: &Path,
    base: Option<&SearchConfig>,
) -> Result<SearchConfig, OperationError> {
    let table = table_or_empty(Some(value), context)?;
    reject_unknown(&table, SEARCH_FIELDS, context)?;
    let defaults = base.cloned().unwrap_or_default();
    let dictionary = optional_path(
        &table,
        "dictionary",
        context,
        base_dir,
        defaults.dictionary.as_deref(),
    )?;
    let dictionary_data = if dictionary == defaults.dictionary {
        defaults.dictionary_data
    } else {
        load_search_dictionary(dictionary.as_deref(), context)?
    };
    Ok(SearchConfig {
        dictionary,
        dictionary_data,
        advanced_index_warning_mb: optional_positive_integer(
            &table,
            "advanced_index_warning_mb",
            context,
            defaults.advanced_index_warning_mb,
        )?,
    })
}

fn load_search_dictionary(
    path: Option<&Path>,
    context: &str,
) -> Result<SearchDictionary, OperationError> {
    let Some(path) = path else {
        return Ok(SearchDictionary::default());
    };
    let source = fs::read_to_string(path).map_err(|cause| {
        error(format!(
            "impossibile leggere dizionario {context} in {}: {cause}",
            path.display()
        ))
    })?;
    let value: Value = toml::from_str(&source)
        .map_err(|cause| error(format!("TOML non valido in {}: {cause}", path.display())))?;
    let table = value
        .as_table()
        .ok_or_else(|| error(format!("{} dizionario deve essere una tabella", context)))?;
    let mut unknown: Vec<_> = table
        .keys()
        .filter(|key| !["terms", "months"].contains(&key.as_str()))
        .cloned()
        .collect();
    unknown.sort();
    if !unknown.is_empty() {
        return Err(error(format!(
            "{context} dizionario sezioni sconosciute: {}",
            unknown.join(", ")
        )));
    }
    let terms = parse_dictionary_section(table.get("terms"), &format!("{context}.terms"))?;
    let months = parse_dictionary_section(table.get("months"), &format!("{context}.months"))?;
    Ok(SearchDictionary { terms, months })
}

fn parse_dictionary_section(
    value: Option<&Value>,
    context: &str,
) -> Result<IndexMap<String, Vec<String>>, OperationError> {
    let Some(value) = value else {
        return Ok(IndexMap::new());
    };
    let table = value
        .as_table()
        .ok_or_else(|| error(format!("{context} deve essere una tabella")))?;
    let mut parsed = IndexMap::new();
    for (key, raw_values) in table {
        if key.trim().is_empty() {
            return Err(error(format!(
                "{context} chiavi devono essere stringhe non vuote"
            )));
        }
        let values = raw_values
            .as_array()
            .filter(|values| !values.is_empty())
            .ok_or_else(|| {
                error(format!(
                    "{context}.{key} deve essere una lista non vuota di stringhe"
                ))
            })?;
        let mut expanded = Vec::with_capacity(values.len());
        for value in values {
            let Some(value) = value.as_str().filter(|value| !value.trim().is_empty()) else {
                return Err(error(format!(
                    "{context}.{key} deve contenere solo stringhe non vuote"
                )));
            };
            expanded.push(value.trim().to_owned());
        }
        parsed.insert(key.trim().to_owned(), expanded);
    }
    Ok(parsed)
}
