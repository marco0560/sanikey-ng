//! Caricamento della directory TOML dei metadati curati.

use super::io::load_items;
use super::models::CuratedMetadata;
use super::parse;
use super::validation::validate_curated_metadata;
use super::value::value_error;
use crate::operation::OperationError;
use std::fs;
use std::path::Path;

/// Carica i metadati curati di una sola persona.
///
/// I file assenti producono collezioni vuote; i file presenti vengono
/// completamente analizzati e validati prima di restituire il risultato.
/// Chiamare questa funzione separatamente per ciascuna directory persona
/// mantiene distinti gli aggregati e impedisce condivisioni accidentali.
///
/// # Errors
///
/// Restituisce un errore `configuration` se la directory, un file o un
/// riferimento tra metadati non è valido.
pub fn load_curated_metadata(
    metadata_dir: impl AsRef<Path>,
) -> Result<CuratedMetadata, OperationError> {
    let metadata_dir = metadata_dir.as_ref();
    if !metadata_dir.exists() {
        return Ok(CuratedMetadata::default());
    }
    if !metadata_dir.is_dir() {
        return Err(value_error(format!(
            "il percorso dei metadati non è una directory: {}",
            metadata_dir.display()
        )));
    }
    let observations_dir = metadata_dir.join("observations");
    let mut metadata = CuratedMetadata {
        problems: load_items(
            &metadata_dir.join("problems.toml"),
            "problem",
            parse::parse_problem,
        )?,
        medications: load_items(
            &metadata_dir.join("medications.toml"),
            "medication",
            parse::parse_medication,
        )?,
        medication_leaflets: load_items(
            &metadata_dir.join("medication_leaflets.toml"),
            "leaflet",
            parse::parse_leaflet,
        )?,
        medication_leaflet_exclusions: load_items(
            &metadata_dir.join("medication_leaflets.toml"),
            "unavailable",
            parse::parse_exclusion,
        )?,
        therapies: load_items(
            &metadata_dir.join("therapies.toml"),
            "therapy",
            parse::parse_therapy,
        )?,
        procedures: load_items(
            &metadata_dir.join("procedures.toml"),
            "procedure",
            parse::parse_procedure,
        )?,
        observations: load_items(
            &metadata_dir.join("observations.toml"),
            "observation",
            parse::parse_observation,
        )?,
        observation_series: load_items(
            &observations_dir.join("series.toml"),
            "series",
            parse::parse_observation_series,
        )?,
        timeline_events: load_items(
            &metadata_dir.join("timeline_events.toml"),
            "event",
            parse::parse_event,
        )?,
        document_tags: parse::parse_document_tags(&metadata_dir.join("document_tags.toml"))?,
        clinical_summary: parse::parse_summary(&metadata_dir.join("clinical_summary.toml"))?,
        ..CuratedMetadata::default()
    };
    metadata.observation_points = load_observation_points(&observations_dir)?;
    validate_curated_metadata(&metadata, metadata_dir)?;
    Ok(metadata)
}

fn load_observation_points(
    directory: &Path,
) -> Result<Vec<super::models::ObservationPoint>, OperationError> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    if !directory.is_dir() {
        return Err(value_error(format!(
            "directory osservazioni non valida: {}",
            directory.display()
        )));
    }
    let entries = fs::read_dir(directory).map_err(|cause| {
        value_error(format!(
            "impossibile leggere directory osservazioni {}: {cause}",
            directory.display()
        ))
    })?;
    let mut paths = entries
        .map(|entry| {
            entry.map(|entry| entry.path()).map_err(|cause| {
                value_error(format!("directory osservazioni non leggibile: {cause}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut points = Vec::new();
    for path in paths {
        if path.extension().is_none_or(|extension| extension != "toml")
            || matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("series.toml" | "import_state.toml")
            )
        {
            continue;
        }
        points.extend(load_items(&path, "point", parse::parse_observation_point)?);
    }
    Ok(points)
}
