//! Ricerca dei candidati farmaco nel catalogo AIFA.

use crate::logging::{
    Component, TraceField, TraceOmission, TraceValue, redact_secret_assignments, trace_current,
    trace_json_value, trace_sensitive_text,
};
use crate::metadata::{
    CuratedMetadata, Medication, MedicationLeaflet, MedicationLeafletExclusion,
    load_curated_metadata,
};
use crate::operation::{ErrorCode, OperationError};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const API_ROOT: &str = "https://api.aifa.gov.it/aifa-bdf-eif-be/1.0.0/";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_MANIFEST: &str = "medication-leaflets.json";
const REFERENCES_LOCK_FILE: &str = ".medication_leaflets.lock";
static NEXT_REFERENCE_TEMP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

macro_rules! trace_aifa_call {
    ($name:literal, [$($field:expr),* $(,)?], $body:expr, $output:expr) => {{
        let trace = trace_current(Component::Core, $name, || vec![$($field),*]);
        let result = run_aifa_body(|| $body);
        if let Some(trace) = trace {
            let output = $output(&result);
            let _ = trace.finish(TraceValue::Data(serde_json::json!({
                "result": output.to_json(),
            })));
        }
        result
    }};
}

fn run_aifa_body<T>(body: impl FnOnce() -> T) -> T {
    body()
}

fn trace_aifa_json_body(body: &str) -> TraceValue {
    match serde_json::from_str::<Value>(body) {
        Ok(value) => trace_aifa_json_value(&value),
        Err(_) => trace_sensitive_text(body),
    }
}

fn trace_aifa_json_value(value: &Value) -> TraceValue {
    let projected = trace_json_value(value).to_json()["value"].clone();
    let mut secret_values = Vec::new();
    collect_aifa_secret_values(value, &projected, &mut secret_values);
    TraceValue::Data(redact_aifa_derived_values(
        value,
        &projected,
        &secret_values,
    ))
}

fn collect_aifa_secret_values(value: &Value, projected: &Value, secrets: &mut Vec<String>) {
    if projected.get("kind").and_then(Value::as_str) == Some("secret_omitted") {
        collect_aifa_string_values(value, secrets);
        return;
    }
    match (value, projected) {
        (Value::Array(values), Value::Array(projected_values)) => {
            for (value, projected) in values.iter().zip(projected_values) {
                collect_aifa_secret_values(value, projected, secrets);
            }
        }
        (Value::Object(values), Value::Object(projected_values)) => {
            for (key, value) in values {
                if let Some(projected) = projected_values.get(key) {
                    collect_aifa_secret_values(value, projected, secrets);
                }
            }
        }
        _ => {}
    }
}

fn collect_aifa_string_values(value: &Value, secrets: &mut Vec<String>) {
    match value {
        Value::String(value) if !value.is_empty() => secrets.push(value.clone()),
        Value::Array(values) => {
            for value in values {
                collect_aifa_string_values(value, secrets);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_aifa_string_values(value, secrets);
            }
        }
        _ => {}
    }
}

fn redact_aifa_derived_values(value: &Value, projected: &Value, secrets: &[String]) -> Value {
    match (value, projected) {
        (Value::Array(values), Value::Array(projected_values)) => Value::Array(
            values
                .iter()
                .zip(projected_values)
                .map(|(value, projected)| redact_aifa_derived_values(value, projected, secrets))
                .collect(),
        ),
        (Value::Object(values), Value::Object(projected_values)) => Value::Object(
            values
                .iter()
                .filter_map(|(key, value)| {
                    let projected = projected_values.get(key)?;
                    let key_contains_secret = secrets
                        .iter()
                        .any(|secret| key == secret || (secret.len() >= 4 && key.contains(secret)));
                    let key = if key_contains_secret {
                        "[secret omitted]".to_owned()
                    } else {
                        redact_secret_assignments(key)
                    };
                    let value = if projected.get("kind").and_then(Value::as_str)
                        == Some("secret_omitted")
                    {
                        projected.clone()
                    } else {
                        redact_aifa_derived_values(value, projected, secrets)
                    };
                    Some((key, value))
                })
                .collect(),
        ),
        (Value::String(value), _) => {
            if secrets.iter().any(|secret| {
                value == secret.as_str() || (secret.len() >= 4 && value.contains(secret))
            }) {
                TraceValue::Secret.to_json()
            } else {
                projected.clone()
            }
        }
        (_, projected) => projected.clone(),
    }
}

fn trace_medication(medication: &Medication) -> TraceValue {
    trace_json_value(&serde_json::json!({
        "id": medication.id,
        "name": medication.name,
        "active_ingredient": medication.active_ingredient,
        "form": medication.form,
        "strength_per_unit": medication.strength_per_unit,
    }))
}

fn text_contains_identified_secret(value: &str) -> bool {
    redact_secret_assignments(value) != value
}

fn bytes_contain_identified_secret(value: &[u8]) -> bool {
    text_contains_identified_secret(&String::from_utf8_lossy(value))
}

fn request_path_contains_identified_secret(path: &str) -> bool {
    let Some((_, query)) = path.split_once("query=") else {
        return false;
    };
    let encoded = query.split('&').next().unwrap_or_default();
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) else {
                    return false;
                };
                let Ok(byte) = u8::from_str_radix(hex, 16) else {
                    return false;
                };
                decoded.push(byte);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded)
        .ok()
        .is_some_and(|query| text_contains_identified_secret(&query))
}

fn trace_aifa_request_url(url: &str, path: &str) -> TraceValue {
    if request_path_contains_identified_secret(path) {
        TraceValue::Secret
    } else {
        trace_sensitive_text(url)
    }
}

fn json_contains_identified_secret(value: &Value) -> bool {
    trace_json_value(value).to_json() != TraceValue::data(value.clone()).to_json()
}

fn medication_contains_identified_secret(medication: &Medication) -> bool {
    [
        medication.id.as_str(),
        medication.name.as_str(),
        medication.active_ingredient.as_deref().unwrap_or_default(),
        medication.form.as_deref().unwrap_or_default(),
        medication.strength_per_unit.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .any(text_contains_identified_secret)
}

fn date_map_contains_identified_secret(values: &BTreeMap<String, String>) -> bool {
    values.iter().any(|(key, value)| {
        text_contains_identified_secret(key) || text_contains_identified_secret(value)
    })
}

fn trace_reference(reference: &MedicationLeaflet) -> TraceValue {
    trace_json_value(&serde_json::json!({
        "medication_id": reference.medication_id,
        "codice_sis": reference.codice_sis,
        "aic6": reference.aic6,
        "downloaded_at": reference.downloaded_at,
        "source_fingerprint": reference.source_fingerprint,
    }))
}

fn trace_references(references: &[MedicationLeaflet]) -> TraceValue {
    TraceValue::Data(serde_json::Value::Array(
        references
            .iter()
            .map(|reference| trace_reference(reference).to_json())
            .collect(),
    ))
}

fn references_contain_identified_secret(references: &[MedicationLeaflet]) -> bool {
    references.iter().any(|reference| {
        [
            reference.medication_id.as_str(),
            reference.codice_sis.as_str(),
            reference.aic6.as_str(),
            reference.downloaded_at.as_deref().unwrap_or_default(),
            reference.source_fingerprint.as_deref().unwrap_or_default(),
        ]
        .into_iter()
        .any(text_contains_identified_secret)
    })
}

fn trace_candidate(candidate: &AifaCandidate) -> TraceValue {
    trace_json_value(&serde_json::json!({
        "title": candidate.title,
        "codice_sis": candidate.codice_sis,
        "aic6": candidate.aic6,
        "active_ingredients": candidate.active_ingredients,
        "form": candidate.form,
        "strength": candidate.strength,
        "company": candidate.company,
        "atc": candidate.atc,
        "routes": candidate.routes,
        "packages": candidate.packages,
        "mismatches": candidate.mismatches,
    }))
}

fn candidate_contains_identified_secret(candidate: &AifaCandidate) -> bool {
    std::iter::once(candidate.title.as_str())
        .chain(std::iter::once(candidate.codice_sis.as_str()))
        .chain(std::iter::once(candidate.aic6.as_str()))
        .chain(candidate.active_ingredients.iter().map(String::as_str))
        .chain(candidate.form.iter().map(String::as_str))
        .chain(candidate.strength.iter().map(String::as_str))
        .chain(candidate.company.iter().map(String::as_str))
        .chain(candidate.atc.iter().map(String::as_str))
        .chain(candidate.routes.iter().map(String::as_str))
        .chain(candidate.packages.iter().map(String::as_str))
        .chain(candidate.mismatches.iter().map(String::as_str))
        .any(text_contains_identified_secret)
}

fn trace_candidates(candidates: &[AifaCandidate]) -> TraceValue {
    TraceValue::Data(serde_json::Value::Array(
        candidates
            .iter()
            .map(|candidate| trace_candidate(candidate).to_json())
            .collect(),
    ))
}

fn trace_search_result(result: &Option<AifaCandidateSearch>) -> TraceValue {
    result.as_ref().map_or_else(
        || TraceValue::data(serde_json::Value::Null),
        |search| {
            TraceValue::Data(serde_json::json!({
                "exact": trace_candidates(&search.exact).to_json(),
                "review": trace_candidates(&search.review).to_json(),
                "all_candidates": trace_candidates(&search.all_candidates).to_json(),
            }))
        },
    )
}

fn trace_download_failures(failures: &[LeafletDownloadFailure]) -> TraceValue {
    trace_json_value(&serde_json::Value::Array(
        failures
            .iter()
            .map(|failure| {
                serde_json::json!({
                    "medication_id": failure.medication_id,
                    "kind": failure.kind,
                    "url": failure.url,
                    "reason": failure.reason,
                })
            })
            .collect(),
    ))
}

fn trace_download_result(result: &LeafletDownloadResult) -> TraceValue {
    trace_json_value(&serde_json::json!({
        "downloaded_at": result.downloaded_at,
        "failures": trace_download_failures(&result.failures).to_json(),
    }))
}

fn trace_operation_result<T: std::fmt::Debug>(result: &Result<T, OperationError>) -> TraceValue {
    match result {
        Ok(value) => trace_sensitive_text(&format!("{value:#?}")),
        Err(error) => TraceValue::Data(serde_json::json!({
            "state":"error",
            "code":error.code().as_str(),
            "message":trace_sensitive_text(error.message()).to_json(),
        })),
    }
}

/// Modifica confermata da applicare al riferimento AIFA di un farmaco.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfirmedReferenceDecision {
    /// Registra il candidato AIFA verificato.
    Confirm(MedicationLeaflet),
    /// Esclude esplicitamente il farmaco dalla consultazione AIFA.
    MarkNotApplicable(MedicationLeafletExclusion),
}

/// Candidato farmaco restituito dal catalogo AIFA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AifaCandidate {
    /// Denominazione commerciale dichiarata da AIFA.
    pub title: String,
    /// Identificatore dell'organizzazione AIFA.
    pub codice_sis: String,
    /// Codice AIC a sei cifre.
    pub aic6: String,
    /// Principi attivi dichiarati da AIFA.
    pub active_ingredients: Vec<String>,
    /// Forma farmaceutica dichiarata da AIFA.
    pub form: Option<String>,
    /// Dosaggio e forma dichiarati da AIFA.
    pub strength: Option<String>,
    /// Azienda titolare dell'autorizzazione.
    pub company: Option<String>,
    /// Descrizioni ATC dichiarate da AIFA.
    pub atc: Vec<String>,
    /// Vie di somministrazione dichiarate da AIFA.
    pub routes: Vec<String>,
    /// Confezioni disponibili dichiarate da AIFA.
    pub packages: Vec<String>,
    /// Campi curati non confermati che richiedono revisione.
    pub mismatches: Vec<String>,
}

/// Candidati AIFA separati fra corrispondenze esatte e casi da rivedere.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AifaCandidateSearch {
    /// Candidati compatibili con tutti i campi curati compilati.
    pub exact: Vec<AifaCandidate>,
    /// Candidati plausibili che richiedono verifica esplicita.
    pub review: Vec<AifaCandidate>,
    /// Tutti i candidati restituiti dalle richieste riuscite.
    pub all_candidates: Vec<AifaCandidate>,
}

/// Descrive il fallimento del download di un singolo documento AIFA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeafletDownloadFailure {
    /// Identificatore del farmaco curato.
    pub medication_id: String,
    /// Tipo del documento, `FI` oppure `RCP`.
    pub kind: String,
    /// URL pubblico richiesto.
    pub url: String,
    /// Diagnostica breve del fallimento.
    pub reason: String,
}

/// Date e diagnostiche dei documenti AIFA scaricati per una generazione.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LeafletDownloadResult {
    /// Date ISO dei download completi, indicizzate per identificatore farmaco.
    pub downloaded_at: BTreeMap<String, String>,
    /// Fallimenti per singolo FI o RCP.
    pub failures: Vec<LeafletDownloadFailure>,
}

/// Percorsi locali dei fogli disponibili per una consultazione senza rete.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocalLeafletDocuments {
    /// Percorso al foglio illustrativo locale, se presente.
    pub leaflet_pdf: Option<PathBuf>,
    /// Percorso al riassunto delle caratteristiche del prodotto locale, se presente.
    pub rcp_pdf: Option<PathBuf>,
}

/// Crea un riferimento AIFA confermato esplicitamente per il farmaco curato.
#[must_use]
pub fn confirm_aifa_candidate(
    medication: &Medication,
    candidate: &AifaCandidate,
) -> MedicationLeaflet {
    let input_has_secret = medication_contains_identified_secret(medication)
        || candidate_contains_identified_secret(candidate);
    trace_aifa_call!(
        "aifa::confirm_aifa_candidate",
        [
            TraceField::new("medication", trace_medication(medication)),
            TraceField::new("candidate", trace_candidate(candidate)),
        ],
        confirm_aifa_candidate_impl(medication, candidate),
        |result: &MedicationLeaflet| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_reference(result)
            }
        }
    )
}

fn confirm_aifa_candidate_impl(
    medication: &Medication,
    candidate: &AifaCandidate,
) -> MedicationLeaflet {
    trace_aifa_call!(
        "aifa::confirm_aifa_candidate_impl",
        [
            TraceField::new("medication", trace_medication(medication)),
            TraceField::new("candidate", trace_candidate(candidate)),
        ],
        MedicationLeaflet {
            medication_id: medication.id.clone(),
            codice_sis: candidate.codice_sis.clone(),
            aic6: candidate.aic6.clone(),
            downloaded_at: None,
            source_fingerprint: Some(medication_fingerprint(medication)),
        },
        |result: &MedicationLeaflet| {
            if medication_contains_identified_secret(medication)
                || candidate_contains_identified_secret(candidate)
            {
                TraceValue::Secret
            } else {
                trace_reference(result)
            }
        }
    )
}

/// Calcola l'impronta stabile dei campi curati usati per la ricerca AIFA.
#[must_use]
pub fn medication_fingerprint(medication: &Medication) -> String {
    let input_has_secret = medication_contains_identified_secret(medication);
    trace_aifa_call!(
        "aifa::medication_fingerprint",
        [TraceField::new("medication", trace_medication(medication))],
        medication_fingerprint_impl(medication),
        |result: &String| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                TraceValue::data(result.as_str())
            }
        }
    )
}

fn medication_fingerprint_impl(medication: &Medication) -> String {
    trace_aifa_call!(
        "aifa::medication_fingerprint_impl",
        [TraceField::new("medication", trace_medication(medication))],
        {
            let identity = [
                medication.name.as_str(),
                medication.active_ingredient.as_deref().unwrap_or_default(),
                medication.form.as_deref().unwrap_or_default(),
                medication.strength_per_unit.as_deref().unwrap_or_default(),
            ]
            .into_iter()
            .map(|value| value.trim().to_lowercase())
            .collect::<Vec<_>>()
            .join("\u{1f}");
            format!("{:x}", Sha256::digest(identity.as_bytes()))
        },
        |result: &String| {
            if medication_contains_identified_secret(medication) {
                TraceValue::Secret
            } else {
                TraceValue::data(result.as_str())
            }
        }
    )
}

/// Persiste riferimenti AIFA confermati e esclusioni curate in formato TOML.
///
/// Gli elementi sono ordinati per identificatore. La validazione dei riferimenti
/// incrociati viene eseguita dal loader dei metadati durante la rilettura. Il
/// file temporaneo viene sincronizzato prima del rename; la directory padre non
/// viene sincronizzata, quindi la durabilità del rename dopo un crash improvviso
/// non è garantita.
pub fn write_confirmed_references(
    metadata_directory: &Path,
    references: &[MedicationLeaflet],
    exclusions: &[MedicationLeafletExclusion],
) -> Result<PathBuf, OperationError> {
    trace_aifa_call!(
        "aifa::write_confirmed_references",
        [
            TraceField::new("metadata_directory", TraceValue::path(metadata_directory)),
            TraceField::new("references", trace_references(references)),
            TraceField::new(
                "exclusions",
                trace_sensitive_text(&format!("{exclusions:#?}"))
            ),
            TraceField::new("lock_file", TraceValue::data(REFERENCES_LOCK_FILE)),
        ],
        write_confirmed_references_impl(metadata_directory, references, exclusions),
        |result: &Result<PathBuf, OperationError>| match result {
            Ok(path) => TraceValue::path(path),
            Err(error) => TraceValue::Data(serde_json::json!({
                "state":"error",
                "code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    )
}

fn write_confirmed_references_impl(
    metadata_directory: &Path,
    references: &[MedicationLeaflet],
    exclusions: &[MedicationLeafletExclusion],
) -> Result<PathBuf, OperationError> {
    let input_has_secret = references_contain_identified_secret(references)
        || exclusions.iter().any(|exclusion| {
            text_contains_identified_secret(&exclusion.medication_id)
                || text_contains_identified_secret(&exclusion.reason)
        });
    trace_aifa_call!(
        "aifa::write_confirmed_references_impl",
        [
            TraceField::new("metadata_directory", TraceValue::path(metadata_directory)),
            TraceField::new("references", trace_references(references)),
            TraceField::new(
                "exclusions",
                trace_sensitive_text(&format!("{exclusions:#?}"))
            ),
        ],
        {
            fs::create_dir_all(metadata_directory).map_err(|error| {
                aifa_persistence_error(format!("creazione directory metadati: {error}"))
            })?;
            let _lock = lock_confirmed_references(metadata_directory)?;
            write_confirmed_references_unlocked(metadata_directory, references, exclusions)
        },
        |result: &Result<PathBuf, OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                match result {
                    Ok(path) => TraceValue::path(path),
                    Err(error) => TraceValue::Data(serde_json::json!({
                        "state":"error",
                        "code":error.code().as_str(),
                        "message":trace_sensitive_text(error.message()).to_json(),
                    })),
                }
            }
        }
    )
}

/// Aggiorna un solo farmaco preservando le modifiche AIFA concorrenti.
///
/// La funzione ricarica i metadati mentre detiene un lock esclusivo per il
/// file dei riferimenti, confronta l'identità curata e la decisione precedente
/// con la snapshot mostrata all'operatore, applica la decisione al contenuto
/// più recente e sostituisce il TOML con una scrittura atomica. Gli aggiornamenti
/// eseguiti tramite questa API vengono serializzati tra processi. Una modifica
/// esterna del farmaco o del suo riferimento, già presente quando il lock è
/// acquisito, causa un conflitto invece di sovrascrivere la snapshot. Il lock
/// è advisory: un writer che non lo rispetta può ancora modificare il file dopo
/// la rilettura. La sostituzione atomica evita file parziali, ma non offre CAS
/// contro writer esterni non cooperanti.
///
/// # Errors
///
/// Restituisce un errore se la snapshot del farmaco è cambiata, la decisione
/// riguarda un altro farmaco o i metadati non possono essere letti o scritti.
pub fn update_confirmed_reference(
    metadata_directory: &Path,
    medication_id: &str,
    expected_source_fingerprint: &str,
    expected_reference: Option<&MedicationLeaflet>,
    expected_exclusion: Option<&MedicationLeafletExclusion>,
    decision: ConfirmedReferenceDecision,
) -> Result<CuratedMetadata, OperationError> {
    let input_has_secret = text_contains_identified_secret(medication_id)
        || text_contains_identified_secret(expected_source_fingerprint)
        || expected_reference.is_some_and(|reference| {
            references_contain_identified_secret(std::slice::from_ref(reference))
        })
        || expected_exclusion.is_some_and(|exclusion| {
            text_contains_identified_secret(&exclusion.medication_id)
                || text_contains_identified_secret(&exclusion.reason)
        });
    trace_aifa_call!(
        "aifa::update_confirmed_reference",
        [
            TraceField::new("metadata_directory", TraceValue::path(metadata_directory)),
            TraceField::new("medication_id", trace_sensitive_text(medication_id)),
            TraceField::new(
                "expected_source_fingerprint",
                trace_sensitive_text(expected_source_fingerprint)
            ),
            TraceField::new(
                "expected_reference",
                expected_reference
                    .map_or(TraceValue::data(serde_json::Value::Null), trace_reference)
            ),
            TraceField::new(
                "expected_exclusion",
                trace_sensitive_text(&format!("{expected_exclusion:#?}"))
            ),
            TraceField::new("decision", trace_sensitive_text(&format!("{decision:#?}"))),
        ],
        update_confirmed_reference_impl(
            metadata_directory,
            medication_id,
            expected_source_fingerprint,
            expected_reference,
            expected_exclusion,
            decision,
        ),
        |result: &Result<CuratedMetadata, OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_operation_result(result)
            }
        }
    )
}

fn update_confirmed_reference_impl(
    metadata_directory: &Path,
    medication_id: &str,
    expected_source_fingerprint: &str,
    expected_reference: Option<&MedicationLeaflet>,
    expected_exclusion: Option<&MedicationLeafletExclusion>,
    decision: ConfirmedReferenceDecision,
) -> Result<CuratedMetadata, OperationError> {
    let input_has_secret = text_contains_identified_secret(medication_id)
        || text_contains_identified_secret(expected_source_fingerprint)
        || expected_reference.is_some_and(|reference| {
            references_contain_identified_secret(std::slice::from_ref(reference))
        })
        || expected_exclusion.is_some_and(|exclusion| {
            text_contains_identified_secret(&exclusion.medication_id)
                || text_contains_identified_secret(&exclusion.reason)
        })
        || match &decision {
            ConfirmedReferenceDecision::Confirm(reference) => {
                references_contain_identified_secret(std::slice::from_ref(reference))
            }
            ConfirmedReferenceDecision::MarkNotApplicable(exclusion) => {
                text_contains_identified_secret(&exclusion.medication_id)
                    || text_contains_identified_secret(&exclusion.reason)
            }
        };
    trace_aifa_call!(
        "aifa::update_confirmed_reference_impl",
        [
            TraceField::new("metadata_directory", TraceValue::path(metadata_directory)),
            TraceField::new("medication_id", trace_sensitive_text(medication_id)),
            TraceField::new(
                "expected_source_fingerprint",
                trace_sensitive_text(expected_source_fingerprint)
            ),
            TraceField::new(
                "expected_reference",
                expected_reference.map_or(TraceValue::data(Value::Null), trace_reference)
            ),
            TraceField::new(
                "expected_exclusion",
                trace_sensitive_text(&format!("{expected_exclusion:#?}"))
            ),
            TraceField::new("decision", trace_sensitive_text(&format!("{decision:#?}"))),
        ],
        {
            fs::create_dir_all(metadata_directory).map_err(|error| {
                aifa_persistence_error(format!("creazione directory metadati: {error}"))
            })?;
            let _lock = lock_confirmed_references(metadata_directory)?;
            let mut metadata = load_curated_metadata(metadata_directory)?;
            let current_medication = metadata
                .medications
                .iter()
                .find(|item| item.id == medication_id)
                .ok_or_else(|| {
                    aifa_persistence_error(format!("farmaco curato non trovato: {medication_id}"))
                })?;
            if medication_fingerprint(current_medication) != expected_source_fingerprint {
                return Err(aifa_persistence_error(format!(
                    "conflitto AIFA: l'identità curata di {medication_id} è cambiata durante la ricerca"
                )));
            }
            let current_reference = metadata
                .medication_leaflets
                .iter()
                .find(|item| item.medication_id == medication_id);
            let current_exclusion = metadata
                .medication_leaflet_exclusions
                .iter()
                .find(|item| item.medication_id == medication_id);
            if current_reference != expected_reference || current_exclusion != expected_exclusion {
                return Err(aifa_persistence_error(format!(
                    "conflitto AIFA: il riferimento di {medication_id} è stato modificato da un'altra operazione"
                )));
            }

            match decision {
                ConfirmedReferenceDecision::Confirm(reference) => {
                    if reference.medication_id != medication_id {
                        return Err(aifa_persistence_error(format!(
                            "la conferma AIFA non corrisponde al farmaco {medication_id}"
                        )));
                    }
                    metadata
                        .medication_leaflets
                        .retain(|item| item.medication_id != medication_id);
                    metadata.medication_leaflets.push(reference);
                    metadata
                        .medication_leaflet_exclusions
                        .retain(|item| item.medication_id != medication_id);
                }
                ConfirmedReferenceDecision::MarkNotApplicable(exclusion) => {
                    if exclusion.medication_id != medication_id || exclusion.reason != "non_aifa" {
                        return Err(aifa_persistence_error(format!(
                            "l'esclusione AIFA non corrisponde al farmaco {medication_id}"
                        )));
                    }
                    metadata
                        .medication_leaflets
                        .retain(|item| item.medication_id != medication_id);
                    metadata
                        .medication_leaflet_exclusions
                        .retain(|item| item.medication_id != medication_id);
                    metadata.medication_leaflet_exclusions.push(exclusion);
                }
            }
            write_confirmed_references_unlocked(
                metadata_directory,
                &metadata.medication_leaflets,
                &metadata.medication_leaflet_exclusions,
            )?;
            Ok(metadata)
        },
        |result: &Result<CuratedMetadata, OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_sensitive_text(&format!("{result:#?}"))
            }
        }
    )
}

fn write_confirmed_references_unlocked(
    metadata_directory: &Path,
    references: &[MedicationLeaflet],
    exclusions: &[MedicationLeafletExclusion],
) -> Result<PathBuf, OperationError> {
    trace_aifa_call!(
        "aifa::write_confirmed_references_unlocked",
        [
            TraceField::new("metadata_directory", TraceValue::path(metadata_directory)),
            TraceField::new("references", trace_references(references)),
            TraceField::new(
                "exclusions",
                trace_sensitive_text(&format!("{exclusions:#?}"))
            ),
        ],
        write_confirmed_references_unlocked_impl(metadata_directory, references, exclusions),
        |result: &Result<PathBuf, OperationError>| match result {
            Ok(path) => TraceValue::path(path),
            Err(error) => TraceValue::Data(serde_json::json!({
                "state":"error","code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    )
}

fn write_confirmed_references_unlocked_impl(
    metadata_directory: &Path,
    references: &[MedicationLeaflet],
    exclusions: &[MedicationLeafletExclusion],
) -> Result<PathBuf, OperationError> {
    let input_has_secret = references_contain_identified_secret(references)
        || exclusions.iter().any(|exclusion| {
            text_contains_identified_secret(&exclusion.medication_id)
                || text_contains_identified_secret(&exclusion.reason)
        });
    trace_aifa_call!(
        "aifa::write_confirmed_references_unlocked_impl",
        [
            TraceField::new("metadata_directory", TraceValue::path(metadata_directory)),
            TraceField::new("references", trace_references(references)),
            TraceField::new(
                "exclusions",
                trace_sensitive_text(&format!("{exclusions:#?}"))
            ),
        ],
        {
            let mut ordered_references = references.to_vec();
            ordered_references.sort_by(|left, right| left.medication_id.cmp(&right.medication_id));
            let mut ordered_exclusions = exclusions.to_vec();
            ordered_exclusions.sort_by(|left, right| left.medication_id.cmp(&right.medication_id));

            let mut root = toml::Table::new();
            if !ordered_references.is_empty() {
                let values = ordered_references
                    .into_iter()
                    .map(|reference| {
                        let mut table = toml::Table::new();
                        table.insert("medication_id".into(), reference.medication_id.into());
                        table.insert("codice_sis".into(), reference.codice_sis.into());
                        table.insert("aic6".into(), reference.aic6.into());
                        if let Some(fingerprint) = reference.source_fingerprint {
                            table.insert("source_fingerprint".into(), fingerprint.into());
                        }
                        toml::Value::Table(table)
                    })
                    .collect();
                root.insert("leaflet".into(), toml::Value::Array(values));
            }
            if !ordered_exclusions.is_empty() {
                let values = ordered_exclusions
                    .into_iter()
                    .map(|exclusion| {
                        let mut table = toml::Table::new();
                        table.insert("medication_id".into(), exclusion.medication_id.into());
                        table.insert("reason".into(), exclusion.reason.into());
                        toml::Value::Table(table)
                    })
                    .collect();
                root.insert("unavailable".into(), toml::Value::Array(values));
            }
            let serialized = toml::to_string(&toml::Value::Table(root)).map_err(|error| {
                aifa_persistence_error(format!("serializzazione dei riferimenti AIFA: {error}"))
            })?;
            let target = metadata_directory.join("medication_leaflets.toml");
            write_file_atomically(metadata_directory, &target, serialized.as_bytes())?;
            Ok(target)
        },
        |result: &Result<PathBuf, OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                match result {
                    Ok(path) => TraceValue::path(path),
                    Err(error) => TraceValue::Data(serde_json::json!({
                        "state":"error",
                        "code":error.code().as_str(),
                        "message":trace_sensitive_text(error.message()).to_json(),
                    })),
                }
            }
        }
    )
}

fn lock_confirmed_references(metadata_directory: &Path) -> Result<File, OperationError> {
    trace_aifa_call!(
        "aifa::lock_confirmed_references",
        [
            TraceField::new("metadata_directory", TraceValue::path(metadata_directory)),
            TraceField::new(
                "lock_path",
                TraceValue::path(&metadata_directory.join(REFERENCES_LOCK_FILE))
            ),
        ],
        lock_confirmed_references_impl(metadata_directory),
        |result: &Result<File, OperationError>| match result {
            Ok(_) => TraceValue::Data(serde_json::json!({
                "state":"locked",
                "file_handle":TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
            })),
            Err(error) => TraceValue::Data(serde_json::json!({
                "state":"error","code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    )
}

fn lock_confirmed_references_impl(metadata_directory: &Path) -> Result<File, OperationError> {
    trace_aifa_call!(
        "aifa::lock_confirmed_references_impl",
        [
            TraceField::new("metadata_directory", TraceValue::path(metadata_directory)),
            TraceField::new(
                "lock_path",
                TraceValue::path(&metadata_directory.join(REFERENCES_LOCK_FILE))
            ),
        ],
        {
            let lock_path = metadata_directory.join(REFERENCES_LOCK_FILE);
            let mut options = OpenOptions::new();
            options.read(true).write(true).create(true).truncate(false);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let lock = options
                .open(&lock_path)
                .map_err(|error| aifa_persistence_error(format!("apertura lock AIFA: {error}")))?;
            lock.lock().map_err(|error| {
                aifa_persistence_error(format!("lock riferimenti AIFA: {error}"))
            })?;
            Ok(lock)
        },
        |result: &Result<File, OperationError>| match result {
            Ok(_) => TraceValue::Data(serde_json::json!({
                "state":"locked",
                "file_handle":TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
            })),
            Err(error) => TraceValue::Data(serde_json::json!({
                "state":"error",
                "code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    )
}

fn write_file_atomically(
    directory: &Path,
    target: &Path,
    contents: &[u8],
) -> Result<(), OperationError> {
    let text_contents = std::str::from_utf8(contents).ok();
    let input_has_secret = text_contents.is_some_and(text_contains_identified_secret);
    trace_aifa_call!(
        "aifa::write_file_atomically",
        [
            TraceField::new("directory", TraceValue::path(directory)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "contents",
                if input_has_secret {
                    TraceValue::Secret
                } else if let Some(text) = text_contents {
                    trace_sensitive_text(text)
                } else {
                    TraceValue::Data(serde_json::json!(contents))
                }
            ),
        ],
        write_file_atomically_impl(directory, target, contents),
        |result: &Result<(), OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_operation_result(result)
            }
        }
    )
}

fn write_file_atomically_impl(
    directory: &Path,
    target: &Path,
    contents: &[u8],
) -> Result<(), OperationError> {
    let text_contents = std::str::from_utf8(contents).ok();
    let input_has_secret = text_contents.is_some_and(text_contains_identified_secret);
    trace_aifa_call!(
        "aifa::write_file_atomically_impl",
        [
            TraceField::new("directory", TraceValue::path(directory)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "contents",
                if input_has_secret {
                    TraceValue::Secret
                } else if let Some(text) = text_contents {
                    trace_sensitive_text(text)
                } else {
                    TraceValue::Data(serde_json::json!(contents))
                }
            ),
        ],
        {
            #[cfg(unix)]
            let target_mode = match fs::metadata(target) {
                Ok(metadata) => {
                    use std::os::unix::fs::PermissionsExt;
                    Some(metadata.permissions().mode() & 0o7777)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(aifa_persistence_error(format!(
                        "lettura permessi {}: {error}",
                        target.display()
                    )));
                }
            };
            let (temporary, mut file) = create_private_temp_file(directory)?;
            let result = (|| {
                file.write_all(contents)
                    .map_err(|error| format!("scrittura file temporaneo AIFA: {error}"))?;
                file.sync_all()
                    .map_err(|error| format!("sincronizzazione file temporaneo AIFA: {error}"))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut permissions = file
                        .metadata()
                        .map_err(|error| format!("lettura permessi temporaneo AIFA: {error}"))?
                        .permissions();
                    permissions.set_mode(target_mode.unwrap_or(0o600));
                    file.set_permissions(permissions)
                        .map_err(|error| format!("ripristino permessi TOML AIFA: {error}"))?;
                }
                file.sync_all()
                    .map_err(|error| format!("sincronizzazione permessi TOML AIFA: {error}"))?;
                drop(file);
                fs::rename(&temporary, target)
                    .map_err(|error| format!("sostituzione atomica {}: {error}", target.display()))
            })();
            if let Err(error) = result {
                let _ = fs::remove_file(&temporary);
                return Err(aifa_persistence_error(error));
            }
            Ok(())
        },
        |result: &Result<(), OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_operation_result(result)
            }
        }
    )
}

fn create_private_temp_file(directory: &Path) -> Result<(PathBuf, File), OperationError> {
    trace_aifa_call!(
        "aifa::create_private_temp_file",
        [TraceField::new("directory", TraceValue::path(directory))],
        create_private_temp_file_impl(directory),
        |result: &Result<(PathBuf, File), OperationError>| match result {
            Ok((path, _)) => TraceValue::Data(serde_json::json!({
                "path":TraceValue::path(path).to_json(),
                "file_handle":TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
            })),
            Err(error) => TraceValue::Data(serde_json::json!({
                "state":"error","code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    )
}

fn create_private_temp_file_impl(directory: &Path) -> Result<(PathBuf, File), OperationError> {
    trace_aifa_call!(
        "aifa::create_private_temp_file_impl",
        [TraceField::new("directory", TraceValue::path(directory))],
        {
            for _ in 0..100 {
                let sequence =
                    NEXT_REFERENCE_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path = directory.join(format!(
                    ".medication_leaflets.{}.{}.tmp",
                    std::process::id(),
                    sequence
                ));
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(&path) {
                    Ok(file) => return Ok((path, file)),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(aifa_persistence_error(format!(
                            "creazione file temporaneo AIFA: {error}"
                        )));
                    }
                }
            }
            Err(aifa_persistence_error(
                "impossibile creare un file temporaneo univoco per i riferimenti AIFA".into(),
            ))
        },
        |result: &Result<(PathBuf, File), OperationError>| match result {
            Ok((path, _)) => TraceValue::Data(serde_json::json!({
                "path":TraceValue::path(path).to_json(),
                "file_handle":TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
            })),
            Err(error) => TraceValue::Data(serde_json::json!({
                "state":"error",
                "code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    )
}

fn aifa_persistence_error(message: String) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}

/// Client sincrono per interrogare il catalogo pubblico AIFA.
pub struct AifaClient {
    agent: ureq::Agent,
    api_root: String,
}

/// Fornisce il client AIFA con le impostazioni di rete standard.
impl Default for AifaClient {
    fn default() -> Self {
        trace_aifa_call!(
            "aifa::AifaClient::default",
            [TraceField::new(
                "constructor",
                TraceValue::data("AifaClient::new"),
            )],
            Self::new(),
            |client: &Self| TraceValue::record(&[
                TraceField::new("client_type", TraceValue::data("AifaClient")),
                TraceField::new("api_root", trace_sensitive_text(&client.api_root)),
                TraceField::new("agent", TraceValue::Omitted(TraceOmission::UnsupportedType),),
            ])
        )
    }
}

/// Esegue le richieste e le trasformazioni del client AIFA.
impl AifaClient {
    /// Crea un client con timeout di 30 secondi e TLS rustls.
    #[must_use]
    pub fn new() -> Self {
        trace_aifa_call!(
            "aifa::AifaClient::new",
            [
                TraceField::new("api_root", trace_sensitive_text(API_ROOT)),
                TraceField::new(
                    "connect_timeout_seconds",
                    TraceValue::data(REQUEST_TIMEOUT.as_secs()),
                ),
                TraceField::new(
                    "read_timeout_seconds",
                    TraceValue::data(REQUEST_TIMEOUT.as_secs()),
                ),
                TraceField::new(
                    "write_timeout_seconds",
                    TraceValue::data(REQUEST_TIMEOUT.as_secs()),
                ),
            ],
            {
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(REQUEST_TIMEOUT)
                    .timeout_read(REQUEST_TIMEOUT)
                    .timeout_write(REQUEST_TIMEOUT)
                    .build();
                Self {
                    agent,
                    api_root: API_ROOT.into(),
                }
            },
            |client: &Self| TraceValue::record(&[
                TraceField::new("client_type", TraceValue::data("AifaClient")),
                TraceField::new("api_root", trace_sensitive_text(&client.api_root)),
                TraceField::new("agent", TraceValue::Omitted(TraceOmission::UnsupportedType),),
            ])
        )
    }

    /// Cerca i farmaci per denominazione commerciale e principio attivo.
    ///
    /// I campi curati pieni devono concordare per entrare in `exact`; le
    /// corrispondenze parziali plausibili restano separate in `review`.
    /// Un risultato `None` significa che tutte le richieste al servizio sono
    /// fallite; una risposta valida senza risultati produce un esito vuoto.
    #[must_use]
    pub fn lookup_candidate_search(
        &self,
        medication: &Medication,
        query: Option<&str>,
    ) -> Option<AifaCandidateSearch> {
        let input_has_secret = medication_contains_identified_secret(medication)
            || query.is_some_and(text_contains_identified_secret);
        let mut payload_has_secret = false;
        trace_aifa_call!(
            "aifa::AifaClient::lookup_candidate_search",
            [
                TraceField::new("medication", trace_medication(medication)),
                TraceField::new(
                    "query",
                    query.map_or_else(
                        || TraceValue::data(serde_json::Value::Null),
                        trace_sensitive_text,
                    ),
                ),
                TraceField::new("api_root", TraceValue::data(self.api_root.as_str())),
            ],
            {
                let (result, has_secret) =
                    self.lookup_candidate_search_classified(medication, query);
                payload_has_secret = has_secret;
                result
            },
            |result: &Option<AifaCandidateSearch>| {
                if input_has_secret || payload_has_secret {
                    TraceValue::Secret
                } else {
                    trace_search_result(result)
                }
            }
        )
    }

    fn lookup_candidate_search_classified(
        &self,
        medication: &Medication,
        query: Option<&str>,
    ) -> (Option<AifaCandidateSearch>, bool) {
        lookup_candidate_search_with_classification(medication, query, |path| self.get_json(path))
    }

    fn get_json(&self, path: &str) -> Option<Value> {
        let url = format!("{}{path}", self.api_root);
        let input_has_secret = request_path_contains_identified_secret(path);
        let trace = trace_current(Component::Core, "aifa::AifaClient::get_json", || {
            vec![
                TraceField::new("url", trace_aifa_request_url(&url, path)),
                TraceField::new("accept_header", TraceValue::data("application/json")),
                TraceField::new("authorization_header_present", TraceValue::data(false)),
                TraceField::new(
                    "http_transport_internals",
                    TraceValue::Omitted(TraceOmission::ExternalComponent),
                ),
            ]
        });
        let mut response_body = None;
        let mut failure_stage = None;
        let result = (|| {
            let response = match self
                .agent
                .get(&url)
                .set("Accept", "application/json")
                .call()
            {
                Ok(response) => response,
                Err(_) => {
                    failure_stage = Some("request_failed");
                    return None;
                }
            };
            let body = match response.into_string() {
                Ok(body) => body,
                Err(_) => {
                    failure_stage = Some("response_body_read_failed");
                    return None;
                }
            };
            let payload: Value = match serde_json::from_str(&body) {
                Ok(payload) => payload,
                Err(_) => {
                    if trace.is_some() {
                        response_body = Some(body);
                    }
                    failure_stage = Some("json_decode_failed");
                    return None;
                }
            };
            if trace.is_some() {
                response_body = Some(body);
            }
            if payload.is_object() {
                Some(payload)
            } else {
                failure_stage = Some("response_not_object");
                None
            }
        })();
        if let Some(trace) = trace {
            let output = if input_has_secret {
                TraceValue::Secret
            } else {
                TraceValue::Data(serde_json::json!({
                    "outcome": if result.is_some() { "succeeded" } else { "unavailable" },
                    "failure_stage": failure_stage,
                    "response_body": response_body.map(|body| trace_aifa_json_body(&body).to_json()),
                    "response_wire_bytes": TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
                    "parsed_payload": result
                        .as_ref()
                        .map(|payload| trace_aifa_json_value(payload).to_json()),
                }))
            };
            let _ = trace.finish(output);
        }
        result
    }

    fn get_pdf(&self, url: &str) -> Result<Vec<u8>, String> {
        let url_has_secret = text_contains_identified_secret(url);
        trace_aifa_call!(
            "aifa::AifaClient::get_pdf",
            [
                TraceField::new("url", trace_sensitive_text(url)),
                TraceField::new("accept_header", TraceValue::data("application/pdf")),
                TraceField::new("authorization_header_present", TraceValue::data(false)),
                TraceField::new(
                    "http_transport_internals",
                    TraceValue::Omitted(TraceOmission::ExternalComponent)
                ),
            ],
            {
                let response = self
                    .agent
                    .get(url)
                    .set("Accept", "application/pdf")
                    .call()
                    .map_err(aifa_request_error)?;
                let mut content = Vec::new();
                response
                    .into_reader()
                    .read_to_end(&mut content)
                    .map_err(|error| format!("errore di rete: {error}"))?;
                Ok(content)
            },
            |result: &Result<Vec<u8>, String>| {
                if url_has_secret
                    || result
                        .as_ref()
                        .is_ok_and(|bytes| bytes_contain_identified_secret(bytes))
                {
                    return TraceValue::Secret;
                }
                match result {
                    Ok(bytes) => TraceValue::Data(serde_json::json!({
                        "state":"received",
                        "body_bytes":bytes,
                        "byte_count":bytes.len(),
                    })),
                    Err(error) => TraceValue::Data(serde_json::json!({
                        "state":"failed",
                        "message":trace_sensitive_text(error).to_json(),
                    })),
                }
            }
        )
    }
}

/// Verifica online la disponibilità di entrambi i documenti per un riferimento.
pub fn probe_leaflet_documents(reference: &MedicationLeaflet) -> Vec<LeafletDownloadFailure> {
    let input_has_secret = references_contain_identified_secret(std::slice::from_ref(reference));
    trace_aifa_call!(
        "aifa::probe_leaflet_documents",
        [
            TraceField::new("reference", trace_reference(reference)),
            TraceField::new(
                "network_transport_internals",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        {
            let client = AifaClient::new();
            fetch_leaflet_documents_with(reference, |url| client.get_pdf(url)).1
        },
        |result: &Vec<LeafletDownloadFailure>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_download_failures(result)
            }
        }
    )
}

/// Scarica i documenti FI/RCP dei riferimenti e aggiorna il manifest locale.
pub fn download_confirmed_leaflets(
    build_root: &Path,
    references: &[MedicationLeaflet],
) -> Result<LeafletDownloadResult, OperationError> {
    let input_has_secret = references_contain_identified_secret(references);
    trace_aifa_call!(
        "aifa::download_confirmed_leaflets",
        [
            TraceField::new("build_root", TraceValue::path(build_root)),
            TraceField::new("references", trace_references(references)),
            TraceField::new(
                "network_transport_internals",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        {
            let client = AifaClient::new();
            download_confirmed_leaflets_with(build_root, references, |url| client.get_pdf(url))
        },
        |result: &Result<LeafletDownloadResult, OperationError>| match result {
            _ if input_has_secret => TraceValue::Secret,
            Ok(result) => trace_download_result(result),
            Err(error) => TraceValue::Data(serde_json::json!({
                "state":"error","code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    )
}

/// Legge le date dei download completi conservate nel manifest locale.
#[must_use]
pub fn leaflet_download_dates(build_root: &Path) -> BTreeMap<String, String> {
    trace_aifa_call!(
        "aifa::leaflet_download_dates",
        [
            TraceField::new("build_root", TraceValue::path(build_root)),
            TraceField::new(
                "manifest_path",
                TraceValue::path(&build_root.join("manifests").join(DOWNLOAD_MANIFEST))
            ),
        ],
        leaflet_download_dates_impl(build_root),
        |result: &BTreeMap<String, String>| {
            if date_map_contains_identified_secret(result) {
                TraceValue::Secret
            } else {
                trace_json_value(&serde_json::json!(result))
            }
        }
    )
}

fn leaflet_download_dates_impl(build_root: &Path) -> BTreeMap<String, String> {
    let path = build_root.join("manifests").join(DOWNLOAD_MANIFEST);
    let content = fs::read_to_string(&path).ok();
    let payload = content
        .as_deref()
        .and_then(|content| serde_json::from_str::<Value>(content).ok());
    let input_has_secret = content
        .as_deref()
        .is_some_and(text_contains_identified_secret)
        || payload
            .as_ref()
            .is_some_and(json_contains_identified_secret);
    trace_aifa_call!(
        "aifa::leaflet_download_dates_impl",
        [
            TraceField::new("manifest_path", TraceValue::path(&path)),
            TraceField::new(
                "manifest_content",
                content
                    .as_deref()
                    .map_or_else(|| TraceValue::data(Value::Null), trace_sensitive_text)
            )
        ],
        {
            payload
                .as_ref()
                .and_then(|payload| payload.get("downloaded_at"))
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(medication_id, date)| {
                    Some((medication_id.clone(), date.as_str()?.to_owned()))
                })
                .collect()
        },
        |result: &BTreeMap<String, String>| {
            if input_has_secret || date_map_contains_identified_secret(result) {
                TraceValue::Secret
            } else {
                trace_json_value(&serde_json::json!(result))
            }
        }
    )
}

/// Individua i PDF locali senza effettuare richieste di rete.
#[must_use]
pub fn local_leaflet_documents(build_root: &Path, medication_id: &str) -> LocalLeafletDocuments {
    trace_aifa_call!(
        "aifa::local_leaflet_documents",
        [
            TraceField::new("build_root", TraceValue::path(build_root)),
            TraceField::new("medication_id", trace_sensitive_text(medication_id)),
        ],
        local_leaflet_documents_impl(build_root, medication_id),
        |result: &LocalLeafletDocuments| TraceValue::Data(serde_json::json!({
            "leaflet_pdf":result.leaflet_pdf.as_deref().map(|path|TraceValue::path(path).to_json()),
            "rcp_pdf":result.rcp_pdf.as_deref().map(|path|TraceValue::path(path).to_json()),
        }))
    )
}

fn local_leaflet_documents_impl(build_root: &Path, medication_id: &str) -> LocalLeafletDocuments {
    let input_has_secret = text_contains_identified_secret(medication_id);
    trace_aifa_call!(
        "aifa::local_leaflet_documents_impl",
        [
            TraceField::new("build_root", TraceValue::path(build_root)),
            TraceField::new("medication_id", trace_sensitive_text(medication_id)),
        ],
        {
            let directory = build_root.join("medication-leaflets").join(medication_id);
            let path_if_file = |filename: &str| {
                let path = directory.join(filename);
                path.is_file().then_some(path)
            };
            LocalLeafletDocuments {
                leaflet_pdf: path_if_file("foglio-illustrativo.pdf"),
                rcp_pdf: path_if_file("rcp.pdf"),
            }
        },
        |result: &LocalLeafletDocuments| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                TraceValue::Data(serde_json::json!({
                    "leaflet_pdf":result.leaflet_pdf.as_deref().map(|path|TraceValue::path(path).to_json()),
                    "rcp_pdf":result.rcp_pdf.as_deref().map(|path|TraceValue::path(path).to_json()),
                }))
            }
        }
    )
}

fn download_confirmed_leaflets_with<F>(
    build_root: &Path,
    references: &[MedicationLeaflet],
    fetch_pdf: F,
) -> Result<LeafletDownloadResult, OperationError>
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    let input_has_secret = references_contain_identified_secret(references);
    trace_aifa_call!(
        "aifa::download_confirmed_leaflets_with",
        [
            TraceField::new("build_root", TraceValue::path(build_root)),
            TraceField::new("references", trace_references(references)),
            TraceField::new(
                "network_fetch_closure",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
            TraceField::new(
                "download_policy",
                TraceValue::data("replace only complete FI/RCP pairs")
            ),
        ],
        download_confirmed_leaflets_with_impl(build_root, references, fetch_pdf),
        |result: &Result<LeafletDownloadResult, OperationError>| match result {
            _ if input_has_secret => TraceValue::Secret,
            Ok(result) => trace_download_result(result),
            Err(error) => TraceValue::Data(serde_json::json!({
                "state":"error","code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    )
}

fn download_confirmed_leaflets_with_impl<F>(
    build_root: &Path,
    references: &[MedicationLeaflet],
    mut fetch_pdf: F,
) -> Result<LeafletDownloadResult, OperationError>
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    let input_has_secret = references_contain_identified_secret(references);
    trace_aifa_call!(
        "aifa::download_confirmed_leaflets_with_impl",
        [
            TraceField::new("build_root", TraceValue::path(build_root)),
            TraceField::new("references", trace_references(references)),
            TraceField::new(
                "fetch_closure",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
        ],
        {
            let mut result = LeafletDownloadResult {
                downloaded_at: leaflet_download_dates(build_root),
                failures: Vec::new(),
            };
            let mut changed = false;
            for reference in references {
                let (downloaded, failures) =
                    fetch_leaflet_documents_with(reference, &mut fetch_pdf);
                result.failures.extend(failures);
                if downloaded.len() != 2 {
                    continue;
                }
                let target = build_root
                    .join("medication-leaflets")
                    .join(&reference.medication_id);
                fs::create_dir_all(&target).map_err(|error| {
                    aifa_persistence_error(format!(
                        "creazione directory {}: {error}",
                        target.display()
                    ))
                })?;
                let fi = target.join("foglio-illustrativo.pdf");
                let rcp = target.join("rcp.pdf");
                let fi_temporary = target.join(".foglio-illustrativo.pdf.tmp");
                let rcp_temporary = target.join(".rcp.pdf.tmp");
                write_temporary(
                    &fi_temporary,
                    &downloaded["foglio-illustrativo.pdf"],
                    input_has_secret,
                )?;
                write_temporary(&rcp_temporary, &downloaded["rcp.pdf"], input_has_secret)?;
                fs::rename(&fi_temporary, &fi).map_err(|error| {
                    aifa_persistence_error(format!("sostituzione {}: {error}", fi.display()))
                })?;
                fs::rename(&rcp_temporary, &rcp).map_err(|error| {
                    aifa_persistence_error(format!("sostituzione {}: {error}", rcp.display()))
                })?;
                result.downloaded_at.insert(
                    reference.medication_id.clone(),
                    chrono::Utc::now().date_naive().to_string(),
                );
                changed = true;
            }
            if changed {
                write_download_dates(build_root, &result.downloaded_at)?;
            }
            Ok(result)
        },
        |result: &Result<LeafletDownloadResult, OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                match result {
                    Ok(result) => trace_download_result(result),
                    Err(error) => TraceValue::Data(serde_json::json!({
                        "state":"error",
                        "code":error.code().as_str(),
                        "message":trace_sensitive_text(error.message()).to_json(),
                    })),
                }
            }
        }
    )
}

fn fetch_leaflet_documents_with<F>(
    reference: &MedicationLeaflet,
    fetch_pdf: F,
) -> (BTreeMap<String, Vec<u8>>, Vec<LeafletDownloadFailure>)
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    let input_has_secret = references_contain_identified_secret(std::slice::from_ref(reference));
    trace_aifa_call!(
        "aifa::fetch_leaflet_documents_with",
        [
            TraceField::new("reference", trace_reference(reference)),
            TraceField::new(
                "fetch_closure",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
        ],
        fetch_leaflet_documents_with_impl(reference, fetch_pdf),
        |result: &(BTreeMap<String, Vec<u8>>, Vec<LeafletDownloadFailure>)| {
            if input_has_secret
                || result
                    .0
                    .values()
                    .any(|bytes| bytes_contain_identified_secret(bytes))
            {
                TraceValue::Secret
            } else {
                TraceValue::Data(serde_json::json!({
                    "downloaded":&result.0,
                    "failures":trace_download_failures(&result.1).to_json(),
                }))
            }
        }
    )
}

fn fetch_leaflet_documents_with_impl<F>(
    reference: &MedicationLeaflet,
    mut fetch_pdf: F,
) -> (BTreeMap<String, Vec<u8>>, Vec<LeafletDownloadFailure>)
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    let input_has_secret = references_contain_identified_secret(std::slice::from_ref(reference));
    trace_aifa_call!(
        "aifa::fetch_leaflet_documents_with_impl",
        [
            TraceField::new("reference", trace_reference(reference)),
            TraceField::new(
                "fetch_closure",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
        ],
        {
            let mut downloaded = BTreeMap::new();
            let mut failures = Vec::new();
            for (kind, filename) in [("FI", "foglio-illustrativo.pdf"), ("RCP", "rcp.pdf")] {
                let url = leaflet_document_url(reference, kind);
                match fetch_pdf(&url) {
                    Ok(content) if content.starts_with(b"%PDF-") => {
                        downloaded.insert(filename.to_owned(), content);
                    }
                    Ok(_) => failures.push(LeafletDownloadFailure {
                        medication_id: reference.medication_id.clone(),
                        kind: kind.to_owned(),
                        url,
                        reason: "risposta AIFA non in formato PDF".into(),
                    }),
                    Err(reason) => failures.push(LeafletDownloadFailure {
                        medication_id: reference.medication_id.clone(),
                        kind: kind.to_owned(),
                        url,
                        reason,
                    }),
                }
            }
            (downloaded, failures)
        },
        |result: &(BTreeMap<String, Vec<u8>>, Vec<LeafletDownloadFailure>)| {
            if input_has_secret
                || result
                    .0
                    .values()
                    .any(|bytes| bytes_contain_identified_secret(bytes))
            {
                TraceValue::Secret
            } else {
                TraceValue::Data(serde_json::json!({
                    "downloaded":&result.0,
                    "failures":trace_download_failures(&result.1).to_json(),
                }))
            }
        }
    )
}

fn leaflet_document_url(reference: &MedicationLeaflet, kind: &str) -> String {
    trace_aifa_call!(
        "aifa::leaflet_document_url",
        [
            TraceField::new("reference", trace_reference(reference)),
            TraceField::new("document_kind", TraceValue::data(kind)),
        ],
        format!(
            "{API_ROOT}organizzazione/{}/farmaci/{}/stampati?ts={kind}",
            reference.codice_sis, reference.aic6
        ),
        |result: &String| trace_sensitive_text(result)
    )
}

fn aifa_request_error(error: ureq::Error) -> String {
    trace_aifa_call!(
        "aifa::aifa_request_error",
        [TraceField::new(
            "http_error",
            trace_sensitive_text(&format!("{error:#?}"))
        )],
        match error {
            ureq::Error::Status(status, response) => {
                let detail = response
                    .into_string()
                    .ok()
                    .and_then(|body| aifa_error_detail(&body));
                detail.map_or_else(
                    || format!("HTTP {status}"),
                    |detail| format!("HTTP {status}: {detail}"),
                )
            }
            ureq::Error::Transport(error) => format!("errore di rete: {error}"),
        },
        |result: &String| trace_sensitive_text(result)
    )
}

fn aifa_error_detail(body: &str) -> Option<String> {
    let input_has_secret = text_contains_identified_secret(body);
    trace_aifa_call!(
        "aifa::aifa_error_detail",
        [TraceField::new("response_body", trace_aifa_json_body(body))],
        aifa_error_detail_impl(body),
        |result: &Option<String>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                result.as_deref().map_or_else(
                    || TraceValue::data(serde_json::Value::Null),
                    trace_sensitive_text,
                )
            }
        }
    )
}

fn aifa_error_detail_impl(body: &str) -> Option<String> {
    trace_aifa_call!(
        "aifa::aifa_error_detail_impl",
        [TraceField::new("response_body", trace_aifa_json_body(body))],
        {
            let payload: Value = serde_json::from_str(body).ok()?;
            let errors = payload.get("errors").and_then(Value::as_array);
            let detail = errors
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .collect::<Vec<_>>()
                .join("; ");
            if !detail.is_empty() {
                return Some(detail.chars().take(240).collect());
            }
            payload
                .get("description")
                .or_else(|| payload.get("message"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(|message| message.chars().take(240).collect())
        },
        |result: &Option<String>| {
            if text_contains_identified_secret(body) {
                TraceValue::Secret
            } else {
                result.as_deref().map_or_else(
                    || TraceValue::data(serde_json::Value::Null),
                    trace_sensitive_text,
                )
            }
        }
    )
}

fn write_temporary(
    path: &Path,
    content: &[u8],
    source_has_secret: bool,
) -> Result<(), OperationError> {
    let input_has_secret = source_has_secret || bytes_contain_identified_secret(content);
    trace_aifa_call!(
        "aifa::write_temporary",
        [
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new(
                "content_bytes",
                if input_has_secret {
                    TraceValue::Secret
                } else {
                    TraceValue::Data(serde_json::json!(content))
                }
            ),
        ],
        write_temporary_impl(path, content, input_has_secret),
        |result: &Result<(), OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_operation_result(result)
            }
        }
    )
}

fn write_temporary_impl(
    path: &Path,
    content: &[u8],
    source_has_secret: bool,
) -> Result<(), OperationError> {
    let input_has_secret = source_has_secret || bytes_contain_identified_secret(content);
    trace_aifa_call!(
        "aifa::write_temporary_impl",
        [
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new(
                "content_bytes",
                if input_has_secret {
                    TraceValue::Secret
                } else {
                    TraceValue::Data(serde_json::json!(content))
                }
            ),
        ],
        {
            let mut file = File::create(path).map_err(|error| {
                aifa_persistence_error(format!(
                    "creazione file temporaneo {}: {error}",
                    path.display()
                ))
            })?;
            file.write_all(content).map_err(|error| {
                aifa_persistence_error(format!(
                    "scrittura file temporaneo {}: {error}",
                    path.display()
                ))
            })
        },
        |result: &Result<(), OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_operation_result(result)
            }
        }
    )
}

fn write_download_dates(
    build_root: &Path,
    downloaded_at: &BTreeMap<String, String>,
) -> Result<(), OperationError> {
    let input_has_secret = date_map_contains_identified_secret(downloaded_at);
    trace_aifa_call!(
        "aifa::write_download_dates",
        [
            TraceField::new("build_root", TraceValue::path(build_root)),
            TraceField::new(
                "downloaded_at",
                if input_has_secret {
                    TraceValue::Secret
                } else {
                    trace_json_value(&serde_json::json!(downloaded_at))
                }
            ),
        ],
        write_download_dates_impl(build_root, downloaded_at),
        |result: &Result<(), OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_operation_result(result)
            }
        }
    )
}

fn write_download_dates_impl(
    build_root: &Path,
    downloaded_at: &BTreeMap<String, String>,
) -> Result<(), OperationError> {
    let input_has_secret = date_map_contains_identified_secret(downloaded_at);
    trace_aifa_call!(
        "aifa::write_download_dates_impl",
        [
            TraceField::new("build_root", TraceValue::path(build_root)),
            TraceField::new(
                "downloaded_at",
                if input_has_secret {
                    TraceValue::Secret
                } else {
                    trace_json_value(&serde_json::json!(downloaded_at))
                }
            ),
        ],
        {
            let directory = build_root.join("manifests");
            fs::create_dir_all(&directory).map_err(|error| {
                aifa_persistence_error(format!(
                    "creazione directory {}: {error}",
                    directory.display()
                ))
            })?;
            let content =
                serde_json::to_vec_pretty(&serde_json::json!({"downloaded_at": downloaded_at}))
                    .map_err(|error| {
                        aifa_persistence_error(format!("serializzazione manifest: {error}"))
                    })?;
            fs::write(directory.join(DOWNLOAD_MANIFEST), content).map_err(|error| {
                aifa_persistence_error(format!("scrittura manifest download AIFA: {error}"))
            })
        },
        |result: &Result<(), OperationError>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_operation_result(result)
            }
        }
    )
}

/// Cerca candidati usando il client AIFA predefinito.
#[must_use]
pub fn lookup_aifa_candidate_search(
    medication: &Medication,
    query: Option<&str>,
) -> Option<AifaCandidateSearch> {
    let input_has_secret = medication_contains_identified_secret(medication)
        || query.is_some_and(text_contains_identified_secret);
    let mut payload_has_secret = false;
    trace_aifa_call!(
        "aifa::lookup_aifa_candidate_search",
        [
            TraceField::new("medication", trace_medication(medication)),
            TraceField::new(
                "query",
                query.map_or_else(|| TraceValue::data(Value::Null), trace_sensitive_text)
            )
        ],
        {
            let (result, has_secret) =
                AifaClient::new().lookup_candidate_search_classified(medication, query);
            payload_has_secret = has_secret;
            result
        },
        |result: &Option<AifaCandidateSearch>| {
            if input_has_secret || payload_has_secret {
                TraceValue::Secret
            } else {
                trace_search_result(result)
            }
        }
    )
}

/// Restituisce soltanto le corrispondenze esatte, distinguendo servizio
/// irraggiungibile (`None`) da una ricerca valida senza risultati.
#[must_use]
pub fn lookup_aifa_candidates(medication: &Medication) -> Option<Vec<AifaCandidate>> {
    let input_has_secret = medication_contains_identified_secret(medication);
    let mut payload_has_secret = false;
    trace_aifa_call!(
        "aifa::lookup_aifa_candidates",
        [TraceField::new("medication", trace_medication(medication))],
        {
            let (search, has_secret) =
                AifaClient::new().lookup_candidate_search_classified(medication, None);
            payload_has_secret = has_secret;
            Some(search?.exact)
        },
        |result: &Option<Vec<AifaCandidate>>| {
            if input_has_secret || payload_has_secret {
                TraceValue::Secret
            } else {
                result.as_ref().map_or_else(
                    || TraceValue::data(Value::Null),
                    |items| trace_candidates(items),
                )
            }
        }
    )
}

/// Cerca le corrispondenze esatte; in caso di errore di rete restituisce una
/// lista vuota. Per distinguere l'errore da una ricerca senza risultati usare
/// [`lookup_aifa_candidates`].
#[must_use]
pub fn find_aifa_candidates(medication: &Medication) -> Vec<AifaCandidate> {
    let input_has_secret = medication_contains_identified_secret(medication);
    let mut payload_has_secret = false;
    trace_aifa_call!(
        "aifa::find_aifa_candidates",
        [TraceField::new("medication", trace_medication(medication))],
        {
            let (search, has_secret) =
                AifaClient::new().lookup_candidate_search_classified(medication, None);
            payload_has_secret = has_secret;
            search.map_or_else(Vec::new, |search| search.exact)
        },
        |items: &Vec<AifaCandidate>| {
            if input_has_secret || payload_has_secret {
                TraceValue::Secret
            } else {
                trace_candidates(items)
            }
        }
    )
}

fn lookup_queries(medication: &Medication, query: Option<&str>) -> Vec<String> {
    let input_has_secret = medication_contains_identified_secret(medication)
        || query.is_some_and(text_contains_identified_secret);
    trace_aifa_call!(
        "aifa::lookup_queries",
        [
            TraceField::new("medication", trace_medication(medication)),
            TraceField::new(
                "query",
                query.map_or_else(|| TraceValue::data(Value::Null), trace_sensitive_text)
            )
        ],
        {
            let values = query.map_or_else(
                || {
                    vec![
                        Some(medication.name.as_str()),
                        medication.active_ingredient.as_deref(),
                    ]
                },
                |value| vec![Some(value)],
            );
            let mut seen = Vec::new();
            let mut result = Vec::new();
            for value in values.into_iter().flatten() {
                if value.trim().is_empty() {
                    continue;
                }
                let normalized = normalized_text(value, false);
                if !normalized.is_empty() && !seen.contains(&normalized) {
                    seen.push(normalized);
                    result.push(value.trim().to_owned());
                }
            }
            result
        },
        |items: &Vec<String>| {
            if input_has_secret {
                TraceValue::Secret
            } else {
                trace_json_value(&serde_json::json!(items))
            }
        }
    )
}

#[cfg(test)]
fn lookup_candidate_search_with<F>(
    medication: &Medication,
    query: Option<&str>,
    fetch_json: F,
) -> Option<AifaCandidateSearch>
where
    F: FnMut(&str) -> Option<Value>,
{
    lookup_candidate_search_with_classification(medication, query, fetch_json).0
}

fn lookup_candidate_search_with_classification<F>(
    medication: &Medication,
    query: Option<&str>,
    mut fetch_json: F,
) -> (Option<AifaCandidateSearch>, bool)
where
    F: FnMut(&str) -> Option<Value>,
{
    let input_has_secret = medication_contains_identified_secret(medication)
        || query.is_some_and(text_contains_identified_secret);
    let mut payload_has_secret = false;
    trace_aifa_call!(
        "aifa::lookup_candidate_search_with",
        [
            TraceField::new("medication", trace_medication(medication)),
            TraceField::new(
                "query",
                query.map_or_else(
                    || TraceValue::data(serde_json::Value::Null),
                    trace_sensitive_text,
                )
            ),
            TraceField::new(
                "fetch_json_closure",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
            TraceField::new(
                "http_transport_internals",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        (
            lookup_candidate_search_with_impl(medication, query, |path| {
                let payload = fetch_json(path);
                payload_has_secret |= payload
                    .as_ref()
                    .is_some_and(json_contains_identified_secret);
                payload
            }),
            payload_has_secret,
        ),
        |(result, has_secret): &(Option<AifaCandidateSearch>, bool)| {
            if input_has_secret || *has_secret {
                TraceValue::Secret
            } else {
                trace_search_result(result)
            }
        }
    )
}

fn lookup_candidate_search_with_impl<F>(
    medication: &Medication,
    query: Option<&str>,
    mut fetch_json: F,
) -> Option<AifaCandidateSearch>
where
    F: FnMut(&str) -> Option<Value>,
{
    let input_has_secret = medication_contains_identified_secret(medication)
        || query.is_some_and(text_contains_identified_secret);
    let mut payload_has_secret = false;
    trace_aifa_call!(
        "aifa::lookup_candidate_search_with_impl",
        [
            TraceField::new("medication", trace_medication(medication)),
            TraceField::new(
                "query",
                query.map_or_else(|| TraceValue::data(Value::Null), trace_sensitive_text)
            ),
            TraceField::new(
                "fetch_json_closure",
                TraceValue::Omitted(TraceOmission::UnsupportedType)
            ),
        ],
        {
            let queries = lookup_queries(medication, query);
            if queries.is_empty() {
                return Some(AifaCandidateSearch::default());
            }

            let mut candidates = Vec::new();
            let mut available = false;
            for lookup_query in queries {
                let request_has_secret =
                    input_has_secret || text_contains_identified_secret(&lookup_query);
                let path = format!(
                    "formadosaggio/ricerca?query={}&page=0",
                    encode_query(&lookup_query)
                );
                let request_trace = trace_current(Component::Core, "aifa::catalog_request", || {
                    vec![
                        TraceField::new("medication", trace_medication(medication)),
                        TraceField::new(
                            "request_path",
                            if request_has_secret {
                                TraceValue::Secret
                            } else {
                                TraceValue::data(path.as_str())
                            },
                        ),
                        TraceField::new(
                            "response_body_representation",
                            TraceValue::data("parsed_json"),
                        ),
                        TraceField::new(
                            "network_exchange",
                            TraceValue::Omitted(TraceOmission::ExternalComponent),
                        ),
                    ]
                });
                let response = fetch_json(&path);
                let response_has_secret = response
                    .as_ref()
                    .is_some_and(json_contains_identified_secret);
                payload_has_secret |= response_has_secret;
                if let Some(trace) = request_trace {
                    let request_succeeded = response.is_some();
                    let result = if request_has_secret || response_has_secret {
                        TraceValue::Secret
                    } else {
                        TraceValue::Data(serde_json::json!({
                            "response":response
                                .as_ref()
                                .map(|payload| trace_aifa_json_value(payload).to_json()),
                            "request_succeeded":request_succeeded,
                        }))
                    };
                    let _ = trace.finish(result);
                }
                let Some(payload) = response else {
                    continue;
                };
                available = true;
                candidates.extend(candidates_from_payload(&payload));
            }
            if !available {
                return None;
            }

            let mut all_candidates = ordered_candidates(candidates, payload_has_secret);
            for candidate in &mut all_candidates {
                candidate.mismatches =
                    candidate_mismatches(candidate, medication, payload_has_secret);
            }
            let exact = all_candidates
                .iter()
                .filter(|candidate| candidate.mismatches.is_empty())
                .cloned()
                .collect();
            let review = all_candidates
                .iter()
                .filter(|candidate| {
                    !candidate.mismatches.is_empty()
                        && candidate_is_plausible(candidate, medication, payload_has_secret)
                })
                .cloned()
                .collect();
            Some(AifaCandidateSearch {
                exact,
                review,
                all_candidates,
            })
        },
        |result: &Option<AifaCandidateSearch>| {
            if input_has_secret || payload_has_secret {
                TraceValue::Secret
            } else {
                trace_search_result(result)
            }
        }
    )
}

fn candidates_from_payload(payload: &Value) -> Vec<AifaCandidate> {
    trace_aifa_call!(
        "aifa::candidates_from_payload",
        [TraceField::new(
            "aifa_json_payload",
            trace_aifa_json_value(payload)
        )],
        candidates_from_payload_impl(payload),
        |result: &Vec<AifaCandidate>| {
            if json_contains_identified_secret(payload) {
                TraceValue::Secret
            } else {
                trace_candidates(result)
            }
        }
    )
}

fn candidates_from_payload_impl(payload: &Value) -> Vec<AifaCandidate> {
    let payload_has_secret = json_contains_identified_secret(payload);
    trace_aifa_call!(
        "aifa::candidates_from_payload_impl",
        [TraceField::new(
            "aifa_json_payload",
            trace_aifa_json_value(payload)
        )],
        {
            let Some(content) = payload
                .get("data")
                .and_then(|data| data.get("content"))
                .and_then(Value::as_array)
            else {
                return Vec::new();
            };
            content
                .iter()
                .filter_map(|item| {
                    let medicine = item.get("medicinale")?;
                    let title = medicine
                        .get("denominazioneMedicinale")
                        .and_then(|value| json_string(value, payload_has_secret))?;
                    let codice_sis = medicine
                        .get("codiceSis")
                        .and_then(|value| json_string(value, payload_has_secret))?;
                    let aic6 = medicine
                        .get("aic6")
                        .and_then(|value| json_string(value, payload_has_secret))?;
                    Some(AifaCandidate {
                        title,
                        codice_sis,
                        aic6,
                        active_ingredients: string_array(
                            item.get("principiAttiviIt"),
                            payload_has_secret,
                        ),
                        form: optional_string(item.get("formaFarmaceutica"), payload_has_secret),
                        strength: optional_string(
                            item.get("descrizioneFormaDosaggio"),
                            payload_has_secret,
                        ),
                        company: optional_string(
                            medicine.get("aziendaTitolare"),
                            payload_has_secret,
                        ),
                        atc: string_array(item.get("descrizioneAtc"), payload_has_secret),
                        routes: string_array(item.get("vieSomministrazione"), payload_has_secret),
                        packages: item
                            .get("confezioni")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|package| {
                                optional_string(
                                    package.get("denominazionePackage"),
                                    payload_has_secret,
                                )
                            })
                            .collect(),
                        mismatches: Vec::new(),
                    })
                })
                .collect()
        },
        |result: &Vec<AifaCandidate>| {
            if json_contains_identified_secret(payload) {
                TraceValue::Secret
            } else {
                trace_candidates(result)
            }
        }
    )
}

fn json_string(value: &Value, source_has_secret: bool) -> Option<String> {
    trace_aifa_call!(
        "aifa::json_string",
        [TraceField::new(
            "value",
            if source_has_secret {
                TraceValue::Secret
            } else {
                trace_aifa_json_value(value)
            },
        )],
        {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_i64().map(|number| number.to_string()))
                .or_else(|| value.as_u64().map(|number| number.to_string()))
        },
        |result: &Option<String>| {
            if source_has_secret || json_contains_identified_secret(value) {
                TraceValue::Secret
            } else {
                result.as_ref().map_or_else(
                    || TraceValue::data(Value::Null),
                    |text| trace_sensitive_text(text),
                )
            }
        }
    )
}

fn optional_string(value: Option<&Value>, source_has_secret: bool) -> Option<String> {
    trace_aifa_call!(
        "aifa::optional_string",
        [TraceField::new(
            "value",
            value.map_or_else(
                || TraceValue::data(Value::Null),
                |value| {
                    if source_has_secret {
                        TraceValue::Secret
                    } else {
                        trace_aifa_json_value(value)
                    }
                },
            )
        )],
        {
            value
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        },
        |result: &Option<String>| {
            if source_has_secret || value.is_some_and(json_contains_identified_secret) {
                TraceValue::Secret
            } else {
                result.as_ref().map_or_else(
                    || TraceValue::data(Value::Null),
                    |text| trace_sensitive_text(text),
                )
            }
        }
    )
}

fn string_array(value: Option<&Value>, source_has_secret: bool) -> Vec<String> {
    trace_aifa_call!(
        "aifa::string_array",
        [TraceField::new(
            "value",
            value.map_or_else(
                || TraceValue::data(Value::Null),
                |value| {
                    if source_has_secret {
                        TraceValue::Secret
                    } else {
                        trace_aifa_json_value(value)
                    }
                },
            )
        )],
        {
            value
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_str().map(str::trim))
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        },
        |result: &Vec<String>| {
            if source_has_secret || value.is_some_and(json_contains_identified_secret) {
                TraceValue::Secret
            } else {
                trace_json_value(&serde_json::json!(result))
            }
        }
    )
}

fn ordered_candidates(
    mut candidates: Vec<AifaCandidate>,
    source_has_secret: bool,
) -> Vec<AifaCandidate> {
    let input_has_secret =
        source_has_secret || candidates.iter().any(candidate_contains_identified_secret);
    trace_aifa_call!(
        "aifa::ordered_candidates",
        [TraceField::new(
            "candidates",
            if source_has_secret {
                TraceValue::Secret
            } else {
                trace_candidates(&candidates)
            }
        )],
        {
            candidates.sort_by(|left, right| {
                (
                    left.title.to_lowercase(),
                    left.strength.as_deref().unwrap_or_default(),
                    left.form.as_deref().unwrap_or_default(),
                    &left.codice_sis,
                    &left.aic6,
                    &left.active_ingredients,
                    &left.company,
                    &left.atc,
                    &left.routes,
                    &left.packages,
                )
                    .cmp(&(
                        right.title.to_lowercase(),
                        right.strength.as_deref().unwrap_or_default(),
                        right.form.as_deref().unwrap_or_default(),
                        &right.codice_sis,
                        &right.aic6,
                        &right.active_ingredients,
                        &right.company,
                        &right.atc,
                        &right.routes,
                        &right.packages,
                    ))
            });
            candidates.dedup();
            candidates
        },
        |result: &Vec<AifaCandidate>| {
            if input_has_secret || result.iter().any(candidate_contains_identified_secret) {
                TraceValue::Secret
            } else {
                trace_candidates(result)
            }
        }
    )
}

fn candidate_mismatches(
    candidate: &AifaCandidate,
    medication: &Medication,
    source_has_secret: bool,
) -> Vec<String> {
    trace_aifa_call!(
        "aifa::candidate_mismatches",
        [
            TraceField::new(
                "candidate",
                if source_has_secret {
                    TraceValue::Secret
                } else {
                    trace_candidate(candidate)
                },
            ),
            TraceField::new("medication", trace_medication(medication))
        ],
        {
            let mut mismatches = Vec::new();
            if medication
                .active_ingredient
                .as_ref()
                .is_some_and(|value| !value.is_empty())
                && !active_ingredients_match(candidate, medication, source_has_secret)
            {
                mismatches.push("principio attivo".into());
            }
            if medication
                .form
                .as_ref()
                .is_some_and(|value| !value.is_empty())
                && !candidate.form.as_ref().is_some_and(|form| {
                    !form_stems(form, source_has_secret).is_disjoint(&form_stems(
                        medication.form.as_deref().unwrap_or_default(),
                        source_has_secret,
                    ))
                })
            {
                mismatches.push("forma".into());
            }
            if medication
                .strength_per_unit
                .as_ref()
                .is_some_and(|value| !value.is_empty())
                && !candidate.strength.as_ref().is_some_and(|strength| {
                    dose_signatures(
                        medication.strength_per_unit.as_deref().unwrap_or_default(),
                        source_has_secret,
                    )
                    .is_subset(&dose_signatures(strength, source_has_secret))
                })
            {
                mismatches.push("dosaggio".into());
            }
            mismatches
        },
        |result: &Vec<String>| {
            if source_has_secret
                || candidate_contains_identified_secret(candidate)
                || medication_contains_identified_secret(medication)
            {
                TraceValue::Secret
            } else {
                trace_json_value(&serde_json::json!(result))
            }
        }
    )
}

fn active_ingredients_match(
    candidate: &AifaCandidate,
    medication: &Medication,
    source_has_secret: bool,
) -> bool {
    trace_aifa_call!(
        "aifa::active_ingredients_match",
        [
            TraceField::new(
                "candidate",
                if source_has_secret {
                    TraceValue::Secret
                } else {
                    trace_candidate(candidate)
                },
            ),
            TraceField::new("medication", trace_medication(medication))
        ],
        {
            let Some(active_ingredient) = medication.active_ingredient.as_deref() else {
                return false;
            };
            let components = active_ingredient_components(active_ingredient, source_has_secret);
            let haystack = candidate
                .active_ingredients
                .iter()
                .map(|item| normalized_text(item, source_has_secret))
                .collect::<Vec<_>>()
                .join(" ");
            !components.is_empty()
                && !haystack.is_empty()
                && components
                    .iter()
                    .all(|component| haystack.contains(component))
        },
        |result: &bool| {
            if source_has_secret
                || candidate_contains_identified_secret(candidate)
                || medication_contains_identified_secret(medication)
            {
                TraceValue::Secret
            } else {
                TraceValue::data(*result)
            }
        }
    )
}

fn active_ingredient_components(value: &str, source_has_secret: bool) -> Vec<String> {
    trace_aifa_call!(
        "aifa::active_ingredient_components",
        [TraceField::new(
            "value",
            if source_has_secret {
                TraceValue::Secret
            } else {
                trace_sensitive_text(value)
            },
        )],
        {
            static SEPARATOR: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
            SEPARATOR
                .get_or_init(|| {
                    Regex::new(r"(?i)\s*(?:,|\+|/|\be\b)\s*")
                        .expect("separatore principi attivi valido")
                })
                .split(value)
                .map(|value| normalized_text(value, source_has_secret))
                .filter(|item| !item.is_empty())
                .collect()
        },
        |result: &Vec<String>| {
            if source_has_secret || text_contains_identified_secret(value) {
                TraceValue::Secret
            } else {
                trace_json_value(&serde_json::json!(result))
            }
        }
    )
}

fn candidate_is_plausible(
    candidate: &AifaCandidate,
    medication: &Medication,
    source_has_secret: bool,
) -> bool {
    trace_aifa_call!(
        "aifa::candidate_is_plausible",
        [
            TraceField::new(
                "candidate",
                if source_has_secret {
                    TraceValue::Secret
                } else {
                    trace_candidate(candidate)
                },
            ),
            TraceField::new("medication", trace_medication(medication))
        ],
        {
            active_ingredients_match(candidate, medication, source_has_secret)
                || commercial_names_match(&candidate.title, &medication.name, source_has_secret)
        },
        |result: &bool| {
            if source_has_secret
                || candidate_contains_identified_secret(candidate)
                || medication_contains_identified_secret(medication)
            {
                TraceValue::Secret
            } else {
                TraceValue::data(*result)
            }
        }
    )
}

fn commercial_names_match(
    candidate_name: &str,
    medication_name: &str,
    source_has_secret: bool,
) -> bool {
    trace_aifa_call!(
        "aifa::commercial_names_match",
        [
            TraceField::new(
                "candidate_name",
                if source_has_secret {
                    TraceValue::Secret
                } else {
                    trace_sensitive_text(candidate_name)
                },
            ),
            TraceField::new(
                "medication_name",
                if source_has_secret {
                    TraceValue::Secret
                } else {
                    trace_sensitive_text(medication_name)
                },
            )
        ],
        {
            let candidate = normalized_text(candidate_name, source_has_secret);
            let medication = normalized_text(medication_name, source_has_secret);
            candidate.len() >= 4
                && !medication.is_empty()
                && (candidate.contains(&medication) || medication.contains(&candidate))
        },
        |result: &bool| {
            if source_has_secret
                || text_contains_identified_secret(candidate_name)
                || text_contains_identified_secret(medication_name)
            {
                TraceValue::Secret
            } else {
                TraceValue::data(*result)
            }
        }
    )
}

fn normalized_text(value: &str, source_has_secret: bool) -> String {
    trace_aifa_call!(
        "aifa::normalized_text",
        [TraceField::new(
            "value",
            if source_has_secret {
                TraceValue::Secret
            } else {
                trace_sensitive_text(value)
            },
        )],
        {
            let mut output = String::new();
            let mut needs_space = false;
            for character in value.chars().flat_map(char::to_lowercase) {
                if character.is_ascii_alphanumeric() {
                    if needs_space && !output.is_empty() {
                        output.push(' ');
                    }
                    output.push(character);
                    needs_space = false;
                } else {
                    needs_space = true;
                }
            }
            output
        },
        |result: &String| if source_has_secret || redact_secret_assignments(value) != value {
            TraceValue::Secret
        } else {
            trace_sensitive_text(result)
        }
    )
}

fn form_stems(value: &str, source_has_secret: bool) -> BTreeSet<String> {
    trace_aifa_call!(
        "aifa::form_stems",
        [TraceField::new(
            "value",
            if source_has_secret {
                TraceValue::Secret
            } else {
                trace_sensitive_text(value)
            },
        )],
        {
            normalized_text(value, source_has_secret)
                .split_whitespace()
                .filter_map(|token| {
                    let stem = token.trim_end_matches(['a', 'e', 'i', 'o', 'p']);
                    let stem = stem.chars().take(8).collect::<String>();
                    (stem.chars().count() >= 4).then_some(stem)
                })
                .collect()
        },
        |result: &BTreeSet<String>| {
            if source_has_secret || text_contains_identified_secret(value) {
                TraceValue::Secret
            } else {
                trace_json_value(&serde_json::json!(result))
            }
        }
    )
}

fn dose_signatures(value: &str, source_has_secret: bool) -> BTreeSet<(String, String)> {
    trace_aifa_call!(
        "aifa::dose_signatures",
        [TraceField::new(
            "value",
            if source_has_secret {
                TraceValue::Secret
            } else {
                trace_sensitive_text(value)
            },
        )],
        {
            static DOSE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
            let pattern = DOSE.get_or_init(|| {
                Regex::new(r"(?i)(\d+(?:[.,]\d+)?)\s*(microgrammi|microgrammo|mcg|mg|ml|ui|g)\b")
                    .expect("grammatica dosaggio AIFA valida")
            });
            pattern
                .captures_iter(value)
                .filter_map(|item| {
                    let number = item.get(1)?.as_str().replace(',', ".");
                    let normalized_number = number.trim_start_matches('0');
                    let number = if normalized_number.is_empty() {
                        "0"
                    } else {
                        normalized_number
                    };
                    let unit = item.get(2)?.as_str().to_lowercase();
                    let unit = match unit.as_str() {
                        "microgrammi" | "microgrammo" | "mcg" => "mcg",
                        "mg" => "mg",
                        "g" => "g",
                        "ml" => "ml",
                        "ui" => "ui",
                        _ => return None,
                    };
                    Some((number.to_owned(), unit.to_owned()))
                })
                .collect()
        },
        |result: &BTreeSet<(String, String)>| {
            if source_has_secret || text_contains_identified_secret(value) {
                TraceValue::Secret
            } else {
                trace_json_value(&serde_json::json!(result))
            }
        }
    )
}

fn encode_query(value: &str) -> String {
    trace_aifa_call!(
        "aifa::encode_query",
        [TraceField::new("value", trace_sensitive_text(value))],
        {
            let mut encoded = String::new();
            for byte in value.bytes() {
                match byte {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        encoded.push(char::from(byte));
                    }
                    b' ' => encoded.push('+'),
                    _ => encoded.push_str(&format!("%{byte:02X}")),
                }
            }
            encoded
        },
        |result: &String| if redact_secret_assignments(value) != value {
            TraceValue::Secret
        } else {
            trace_sensitive_text(result)
        }
    )
}

#[cfg(test)]
mod tests;
