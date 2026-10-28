//! Metadati curati e modelli di dominio indipendenti per persona.

mod io;
mod load;
mod models;
mod parse;
mod preflight;
mod validation;
mod value;

#[cfg(test)]
mod tests;

pub use load::load_curated_metadata;
pub use models::{
    ClinicalProblem, CuratedMetadata, DocumentRecord, Medication, MedicationLeaflet,
    MedicationLeafletExclusion, Observation, ObservationPoint, ObservationSeries, Procedure,
    Provenance, TherapyEpisode, TimelineEvent,
};
pub use preflight::{PersonMetadata, prepare_people_metadata};
