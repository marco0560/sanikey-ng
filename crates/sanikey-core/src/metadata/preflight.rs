//! Preparazione completa dei metadati prima del lavoro sui documenti.

use super::load::load_curated_metadata;
use super::models::CuratedMetadata;
use super::{trace_metadata_call, trace_metadata_result};
use crate::config::AccountsConfig;
use crate::logging::{TraceField, TraceValue};
use crate::operation::OperationError;
use crate::privacy::validate_runtime_privacy;
use std::path::Path;

/// Metadati validati e associati a una sola persona abilitata.
#[derive(Clone, Debug, PartialEq)]
pub struct PersonMetadata {
    /// Identificatore tecnico della persona.
    pub person_id: String,
    /// Aggregato indipendente caricato dalla sua directory metadata.
    pub metadata: CuratedMetadata,
}

/// Valida privacy e metadati di tutte le persone abilitate prima di elaborare.
///
/// Il vettore viene restituito solo se ogni persona supera il preflight: un
/// errore non espone risultati parziali a una fase di elaborazione successiva.
/// Non esegue controlli Git; la scansione dei contenuti candidati alla
/// pubblicazione è riservata agli strumenti di sviluppo.
///
/// # Errors
///
/// Restituisce il primo errore privacy o configuration rilevato.
pub fn prepare_people_metadata(
    config: &AccountsConfig,
    repo_root: impl AsRef<Path>,
) -> Result<Vec<PersonMetadata>, OperationError> {
    let repo_root = repo_root.as_ref();
    trace_metadata_call(
        "metadata::prepare_people_metadata",
        || {
            let mut fields = vec![
                TraceField::new("repo_root", TraceValue::path(repo_root)),
                TraceField::new("config_path", TraceValue::path(&config.path)),
            ];
            for person in config.enabled_people() {
                fields.push(TraceField::new(
                    "person_id",
                    TraceValue::data(person.id.as_str()),
                ));
                fields.push(TraceField::new(
                    "metadata_directory",
                    TraceValue::path(&person.metadata_directory),
                ));
            }
            fields
        },
        || {
            validate_runtime_privacy(config, repo_root)?;
            config
                .enabled_people()
                .map(|person| {
                    Ok(PersonMetadata {
                        person_id: person.id.clone(),
                        metadata: load_curated_metadata(&person.metadata_directory)?,
                    })
                })
                .collect()
        },
        trace_metadata_result,
    )
}
