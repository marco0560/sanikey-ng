//! Parser per problemi, farmaci, terapie e osservazioni curate.

use super::super::models::{
    ClinicalProblem, Medication, MedicationLeaflet, MedicationLeafletExclusion, Observation,
    Procedure, TherapyEpisode,
};
use super::super::value::{optional_string, required_string, value_error};
use crate::operation::OperationError;
use std::path::Path;
use toml::Table;

pub(in crate::metadata) fn parse_problem(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<ClinicalProblem, OperationError> {
    Ok(ClinicalProblem {
        id: required_string(item, "id", path, index)?,
        title: required_string(item, "title", path, index)?,
        status: optional_string(item, "status")?.unwrap_or_else(|| "unknown".to_owned()),
        provenance: Vec::new(),
    })
}

pub(in crate::metadata) fn parse_medication(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<Medication, OperationError> {
    Ok(Medication {
        id: required_string(item, "id", path, index)?,
        name: required_string(item, "name", path, index)?,
        active_ingredient: optional_string(item, "active_ingredient")?,
        form: optional_string(item, "form")?,
        strength_per_unit: optional_string(item, "strength_per_unit")?,
    })
}

pub(in crate::metadata) fn parse_leaflet(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<MedicationLeaflet, OperationError> {
    Ok(MedicationLeaflet {
        medication_id: required_string(item, "medication_id", path, index)?,
        codice_sis: required_string(item, "codice_sis", path, index)?,
        aic6: required_string(item, "aic6", path, index)?,
        downloaded_at: optional_string(item, "downloaded_at")?,
        source_fingerprint: optional_string(item, "source_fingerprint")?,
    })
}

pub(in crate::metadata) fn parse_exclusion(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<MedicationLeafletExclusion, OperationError> {
    let reason = optional_string(item, "reason")?.unwrap_or_else(|| "non_aifa".to_owned());
    if reason != "non_aifa" {
        return Err(value_error(format!(
            "{}: elemento {index} reason non supportato: {reason}",
            path.display()
        )));
    }
    Ok(MedicationLeafletExclusion {
        medication_id: required_string(item, "medication_id", path, index)?,
        reason,
    })
}

pub(in crate::metadata) fn parse_therapy(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<TherapyEpisode, OperationError> {
    let medication_id = required_string(item, "medication_id", path, index)?;
    let id = match optional_string(item, "id")? {
        Some(id) => id,
        None => format!("therapy-{medication_id}-{}", index + 1),
    };
    Ok(TherapyEpisode {
        id,
        medication_id,
        start_date: optional_string(item, "start_date")?,
        end_date: optional_string(item, "end_date")?,
        dosage: optional_string(item, "dosage")?,
        role: optional_string(item, "role")?,
        schedule: super::super::value::string_tuple(item.get("schedule"), path, "schedule", index)?,
        instructions: optional_string(item, "instructions")?,
    })
}

pub(in crate::metadata) fn parse_procedure(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<Procedure, OperationError> {
    Ok(Procedure {
        id: required_string(item, "id", path, index)?,
        title: required_string(item, "title", path, index)?,
        date: optional_string(item, "date")?,
        status: optional_string(item, "status")?.unwrap_or_else(|| "unknown".to_owned()),
    })
}

pub(in crate::metadata) fn parse_observation(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<Observation, OperationError> {
    Ok(Observation {
        id: required_string(item, "id", path, index)?,
        kind: required_string(item, "kind", path, index)?,
        value: required_string(item, "value", path, index)?,
        date: optional_string(item, "date")?,
    })
}
