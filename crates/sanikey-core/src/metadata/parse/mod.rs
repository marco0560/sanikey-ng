//! Conversione dei record TOML nei modelli di dominio.

mod clinical;
mod observations;
mod timeline;

use super::{trace_metadata_call, trace_metadata_result};
use crate::logging::{TraceField, TraceValue, trace_toml_table};
use crate::operation::OperationError;
use std::fmt::Debug;
use std::path::Path;
use toml::Table;

pub(super) fn trace_parse_call<T: Debug>(
    function: &'static str,
    item: &Table,
    path: &Path,
    index: usize,
    action: impl FnOnce() -> Result<T, OperationError>,
) -> Result<T, OperationError> {
    trace_metadata_call(
        function,
        || {
            vec![
                TraceField::new("raw_item", trace_toml_table(item)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        action,
        trace_metadata_result,
    )
}

pub(super) use clinical::{
    parse_exclusion, parse_leaflet, parse_medication, parse_observation, parse_problem,
    parse_procedure, parse_therapy,
};
pub(super) use observations::{parse_observation_point, parse_observation_series};
pub(super) use timeline::{parse_document_tags, parse_event, parse_summary};
