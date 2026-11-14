//! Metadati curati e modelli di dominio indipendenti per persona.

mod io;
mod load;
mod models;
mod parse;
mod preflight;
mod validation;
mod value;

use crate::logging::{Component, TraceField, TraceValue, trace_current, trace_sensitive_text};
use crate::operation::OperationError;

/// Esegue una chiamata del loader lasciandola registrabile solo al livello Trace.
pub(super) fn trace_metadata_call<T>(
    function: &'static str,
    arguments: impl FnOnce() -> Vec<TraceField>,
    action: impl FnOnce() -> T,
    result: impl FnOnce(&T) -> TraceValue,
) -> T {
    let trace = trace_current(Component::Core, function, arguments);
    let value = action();
    if let Some(trace) = trace {
        let _ = trace.finish(result(&value));
    }
    value
}

pub(super) fn trace_metadata_result<T: std::fmt::Debug>(
    result: &Result<T, OperationError>,
) -> TraceValue {
    match result {
        Ok(value) => trace_sensitive_text(&format!("{value:#?}")),
        Err(error) => TraceValue::record(&[
            TraceField::new("code", TraceValue::data(error.code().as_str())),
            TraceField::new("message", trace_sensitive_text(error.message())),
        ]),
    }
}

#[cfg(test)]
mod tests;

pub use load::load_curated_metadata;
pub use models::{
    ClinicalProblem, CuratedMetadata, DocumentRecord, Medication, MedicationLeaflet,
    MedicationLeafletExclusion, Observation, ObservationPoint, ObservationSeries, Procedure,
    Provenance, TherapyEpisode, TimelineEvent,
};
pub use preflight::{PersonMetadata, prepare_people_metadata};
