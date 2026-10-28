//! Aggregato immutabile dei metadati curati di una persona.

use super::{
    ClinicalProblem, Medication, MedicationLeaflet, MedicationLeafletExclusion, Observation,
    ObservationPoint, ObservationSeries, Procedure, TherapyEpisode, TimelineEvent,
};
use indexmap::IndexMap;

/// Collezioni caricate dalla directory metadati di una singola persona.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CuratedMetadata {
    /// Problemi clinici curati.
    pub problems: Vec<ClinicalProblem>,
    /// Farmaci identificati dalla cartella.
    pub medications: Vec<Medication>,
    /// Riferimenti AIFA confermati.
    pub medication_leaflets: Vec<MedicationLeaflet>,
    /// Farmaci esplicitamente esclusi dalla ricerca AIFA.
    pub medication_leaflet_exclusions: Vec<MedicationLeafletExclusion>,
    /// Episodi di terapia.
    pub therapies: Vec<TherapyEpisode>,
    /// Procedure cliniche.
    pub procedures: Vec<Procedure>,
    /// Osservazioni curate.
    pub observations: Vec<Observation>,
    /// Serie longitudinali normalizzate.
    pub observation_series: Vec<ObservationSeries>,
    /// Punti longitudinali normalizzati.
    pub observation_points: Vec<ObservationPoint>,
    /// Eventi manuali della cronologia.
    pub timeline_events: Vec<TimelineEvent>,
    /// Tag indicizzati per nome file o digest.
    pub document_tags: IndexMap<String, Vec<String>>,
    /// Sintesi clinica curata.
    pub clinical_summary: Option<String>,
}
