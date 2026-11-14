//! Parser per cronologia, tag documentali e sintesi.

use super::super::models::TimelineEvent;
use super::super::value::{optional_string, required_string, string_tuple, value_error};
use super::super::{trace_metadata_call, trace_metadata_result};
use super::trace_parse_call;
use crate::logging::{TraceField, TraceValue};
use crate::operation::OperationError;
use indexmap::IndexMap;
use std::path::Path;
use toml::{Table, Value};

pub(in crate::metadata) fn parse_event(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<TimelineEvent, OperationError> {
    trace_parse_call("metadata::parse_event", item, path, index, || {
        Ok(TimelineEvent {
            id: required_string(item, "id", path, index)?,
            title: required_string(item, "title", path, index)?,
            start_date: optional_string(item, "start_date")?,
            end_date: optional_string(item, "end_date")?,
            source: optional_string(item, "source")?.unwrap_or_else(|| "manual".to_owned()),
            links: string_tuple(item.get("links"), path, "links", index)?,
        })
    })
}

pub(in crate::metadata) fn parse_document_tags(
    path: &Path,
) -> Result<IndexMap<String, Vec<String>>, OperationError> {
    trace_metadata_call(
        "metadata::parse_document_tags",
        || vec![TraceField::new("path", TraceValue::path(path))],
        || {
            let data = super::super::io::load_toml(path)?;
            let Some(tags) = data.get("tags") else {
                return Ok(IndexMap::new());
            };
            let Some(tags) = tags.as_table() else {
                return Err(value_error(format!(
                    "{}: tags deve essere una tabella",
                    path.display()
                )));
            };
            let mut parsed = IndexMap::new();
            for (document, raw_tags) in tags {
                let tag_list = string_tuple(Some(raw_tags), path, &format!("tags.{document}"), 0)?;
                parsed.insert(document.clone(), tag_list);
            }
            Ok(parsed)
        },
        trace_metadata_result,
    )
}

pub(in crate::metadata) fn parse_summary(path: &Path) -> Result<Option<String>, OperationError> {
    trace_metadata_call(
        "metadata::parse_summary",
        || vec![TraceField::new("path", TraceValue::path(path))],
        || {
            let data = super::super::io::load_toml(path)?;
            let Some(summary) = data.get("summary") else {
                return Ok(None);
            };
            match summary {
                Value::String(summary) => Ok(Some(summary.clone())),
                _ => Err(value_error(format!(
                    "{}: summary deve essere una stringa",
                    path.display()
                ))),
            }
        },
        trace_metadata_result,
    )
}
