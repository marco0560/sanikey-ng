//! Validazione di identificatori e riferimenti tra file curati.

use super::models::CuratedMetadata;
use super::value::value_error;
use crate::operation::OperationError;
use std::collections::BTreeSet;
use std::path::Path;

pub(super) fn validate_curated_metadata(
    metadata: &CuratedMetadata,
    metadata_dir: &Path,
) -> Result<(), OperationError> {
    validate_unique_ids(
        "problem",
        metadata.problems.iter().map(|item| item.id.as_str()),
        &metadata_dir.join("problems.toml"),
    )?;
    let excluded = validate_unique_ids(
        "medication_leaflet_unavailable",
        metadata
            .medication_leaflet_exclusions
            .iter()
            .map(|item| item.medication_id.as_str()),
        &metadata_dir.join("medication_leaflets.toml"),
    )?;
    let referenced: BTreeSet<_> = metadata
        .medication_leaflets
        .iter()
        .map(|item| item.medication_id.as_str())
        .collect();
    if let Some(overlap) = referenced.intersection(&excluded).next() {
        return Err(value_error(format!(
            "{}: medication_id con riferimento AIFA e stato non_aifa: {overlap}",
            metadata_dir.join("medication_leaflets.toml").display()
        )));
    }
    let medications = validate_unique_ids(
        "medication",
        metadata.medications.iter().map(|item| item.id.as_str()),
        &metadata_dir.join("medications.toml"),
    )?;
    validate_unique_ids(
        "medication_leaflet",
        metadata
            .medication_leaflets
            .iter()
            .map(|item| item.medication_id.as_str()),
        &metadata_dir.join("medication_leaflets.toml"),
    )?;
    validate_unique_ids(
        "therapy",
        metadata.therapies.iter().map(|item| item.id.as_str()),
        &metadata_dir.join("therapies.toml"),
    )?;
    validate_unique_ids(
        "procedure",
        metadata.procedures.iter().map(|item| item.id.as_str()),
        &metadata_dir.join("procedures.toml"),
    )?;
    validate_unique_ids(
        "observation",
        metadata.observations.iter().map(|item| item.id.as_str()),
        &metadata_dir.join("observations.toml"),
    )?;
    let series = validate_unique_ids(
        "observation_series",
        metadata
            .observation_series
            .iter()
            .map(|item| item.id.as_str()),
        &metadata_dir.join("observations/series.toml"),
    )?;
    validate_unique_ids(
        "observation_point",
        metadata
            .observation_points
            .iter()
            .map(|item| item.id.as_str()),
        &metadata_dir.join("observations"),
    )?;
    for point in &metadata.observation_points {
        if !series.contains(point.series_id.as_str()) {
            return Err(value_error(format!(
                "{}: observation point {} referenzia series_id sconosciuto {}",
                metadata_dir.join("observations").display(),
                point.id,
                point.series_id
            )));
        }
    }
    validate_unique_ids(
        "event",
        metadata.timeline_events.iter().map(|item| item.id.as_str()),
        &metadata_dir.join("timeline_events.toml"),
    )?;
    for therapy in &metadata.therapies {
        if !medications.contains(therapy.medication_id.as_str()) {
            return Err(value_error(format!(
                "{}: therapy {} referenzia medication_id sconosciuto {}",
                metadata_dir.join("therapies.toml").display(),
                therapy.id,
                therapy.medication_id
            )));
        }
    }
    for leaflet in &metadata.medication_leaflets {
        if !medications.contains(leaflet.medication_id.as_str()) {
            return Err(value_error(format!(
                "{}: riferimento AIFA per medication_id sconosciuto {}",
                metadata_dir.join("medication_leaflets.toml").display(),
                leaflet.medication_id
            )));
        }
    }
    for exclusion in &metadata.medication_leaflet_exclusions {
        if !medications.contains(exclusion.medication_id.as_str()) {
            return Err(value_error(format!(
                "{}: stato non_aifa per medication_id sconosciuto {}",
                metadata_dir.join("medication_leaflets.toml").display(),
                exclusion.medication_id
            )));
        }
    }
    Ok(())
}

fn validate_unique_ids<'a>(
    label: &str,
    ids: impl IntoIterator<Item = &'a str>,
    path: &Path,
) -> Result<BTreeSet<&'a str>, OperationError> {
    let mut seen = BTreeSet::new();
    for item_id in ids {
        if !seen.insert(item_id) {
            return Err(value_error(format!(
                "{}: id {label} duplicato: {item_id}",
                path.display()
            )));
        }
    }
    Ok(seen)
}
