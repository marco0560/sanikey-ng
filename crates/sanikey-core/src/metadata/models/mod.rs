//! Tipi di dominio dei metadati.

mod collection;
mod documents;
mod entities;
mod observations;

pub use collection::CuratedMetadata;
pub use documents::{DocumentRecord, Provenance};
pub use entities::{
    ClinicalProblem, Medication, MedicationLeaflet, MedicationLeafletExclusion, Observation,
    Procedure, TherapyEpisode, TimelineEvent,
};
pub use observations::{ObservationPoint, ObservationSeries};
