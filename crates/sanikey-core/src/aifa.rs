//! Ricerca dei candidati farmaco nel catalogo AIFA.

use crate::metadata::Medication;
use crate::metadata::{MedicationLeaflet, MedicationLeafletExclusion};
use crate::operation::{ErrorCode, OperationError};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const API_ROOT: &str = "https://api.aifa.gov.it/aifa-bdf-eif-be/1.0.0/";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_MANIFEST: &str = "medication-leaflets.json";

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
    MedicationLeaflet {
        medication_id: medication.id.clone(),
        codice_sis: candidate.codice_sis.clone(),
        aic6: candidate.aic6.clone(),
        downloaded_at: None,
        source_fingerprint: Some(medication_fingerprint(medication)),
    }
}

/// Calcola l'impronta stabile dei campi curati usati per la ricerca AIFA.
#[must_use]
pub fn medication_fingerprint(medication: &Medication) -> String {
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
}

/// Persiste riferimenti AIFA confermati e esclusioni curate in formato TOML.
///
/// Gli elementi sono ordinati per identificatore. La validazione dei riferimenti
/// incrociati viene eseguita dal loader dei metadati durante la rilettura.
pub fn write_confirmed_references(
    metadata_directory: &Path,
    references: &[MedicationLeaflet],
    exclusions: &[MedicationLeafletExclusion],
) -> Result<PathBuf, OperationError> {
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
    fs::create_dir_all(metadata_directory).map_err(|error| {
        aifa_persistence_error(format!("creazione directory metadati: {error}"))
    })?;
    fs::write(&target, serialized).map_err(|error| {
        aifa_persistence_error(format!("scrittura {}: {error}", target.display()))
    })?;
    Ok(target)
}

fn aifa_persistence_error(message: String) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}

/// Client sincrono per interrogare il catalogo pubblico AIFA.
pub struct AifaClient {
    agent: ureq::Agent,
    api_root: String,
}

impl Default for AifaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AifaClient {
    /// Crea un client con timeout di 30 secondi e TLS rustls.
    #[must_use]
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(REQUEST_TIMEOUT)
            .timeout_read(REQUEST_TIMEOUT)
            .timeout_write(REQUEST_TIMEOUT)
            .build();
        Self {
            agent,
            api_root: API_ROOT.into(),
        }
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
        lookup_candidate_search_with(medication, query, |path| self.get_json(path))
    }

    fn get_json(&self, path: &str) -> Option<Value> {
        let response = self
            .agent
            .get(&format!("{}{path}", self.api_root))
            .set("Accept", "application/json")
            .call()
            .ok()?;
        let payload: Value = serde_json::from_str(&response.into_string().ok()?).ok()?;
        payload.is_object().then_some(payload)
    }

    fn get_pdf(&self, url: &str) -> Result<Vec<u8>, String> {
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
    }
}

/// Verifica online la disponibilità di entrambi i documenti per un riferimento.
pub fn probe_leaflet_documents(reference: &MedicationLeaflet) -> Vec<LeafletDownloadFailure> {
    let client = AifaClient::new();
    fetch_leaflet_documents_with(reference, |url| client.get_pdf(url)).1
}

/// Scarica i documenti FI/RCP dei riferimenti e aggiorna il manifest locale.
pub fn download_confirmed_leaflets(
    build_root: &Path,
    references: &[MedicationLeaflet],
) -> Result<LeafletDownloadResult, OperationError> {
    let client = AifaClient::new();
    download_confirmed_leaflets_with(build_root, references, |url| client.get_pdf(url))
}

/// Legge le date dei download completi conservate nel manifest locale.
#[must_use]
pub fn leaflet_download_dates(build_root: &Path) -> BTreeMap<String, String> {
    let path = build_root.join("manifests").join(DOWNLOAD_MANIFEST);
    let Ok(content) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(payload) = serde_json::from_str::<Value>(&content) else {
        return BTreeMap::new();
    };
    payload
        .get("downloaded_at")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(medication_id, date)| {
            Some((medication_id.clone(), date.as_str()?.to_owned()))
        })
        .collect()
}

/// Individua i PDF locali senza effettuare richieste di rete.
#[must_use]
pub fn local_leaflet_documents(build_root: &Path, medication_id: &str) -> LocalLeafletDocuments {
    let directory = build_root.join("medication-leaflets").join(medication_id);
    let path_if_file = |filename: &str| {
        let path = directory.join(filename);
        path.is_file().then_some(path)
    };
    LocalLeafletDocuments {
        leaflet_pdf: path_if_file("foglio-illustrativo.pdf"),
        rcp_pdf: path_if_file("rcp.pdf"),
    }
}

fn download_confirmed_leaflets_with<F>(
    build_root: &Path,
    references: &[MedicationLeaflet],
    mut fetch_pdf: F,
) -> Result<LeafletDownloadResult, OperationError>
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    let mut result = LeafletDownloadResult {
        downloaded_at: leaflet_download_dates(build_root),
        failures: Vec::new(),
    };
    let mut changed = false;
    for reference in references {
        let (downloaded, failures) = fetch_leaflet_documents_with(reference, &mut fetch_pdf);
        result.failures.extend(failures);
        if downloaded.len() != 2 {
            continue;
        }
        let target = build_root
            .join("medication-leaflets")
            .join(&reference.medication_id);
        fs::create_dir_all(&target).map_err(|error| {
            aifa_persistence_error(format!("creazione directory {}: {error}", target.display()))
        })?;
        let fi = target.join("foglio-illustrativo.pdf");
        let rcp = target.join("rcp.pdf");
        let fi_temporary = target.join(".foglio-illustrativo.pdf.tmp");
        let rcp_temporary = target.join(".rcp.pdf.tmp");
        write_temporary(&fi_temporary, &downloaded["foglio-illustrativo.pdf"])?;
        write_temporary(&rcp_temporary, &downloaded["rcp.pdf"])?;
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
}

fn fetch_leaflet_documents_with<F>(
    reference: &MedicationLeaflet,
    mut fetch_pdf: F,
) -> (BTreeMap<String, Vec<u8>>, Vec<LeafletDownloadFailure>)
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
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
}

fn leaflet_document_url(reference: &MedicationLeaflet, kind: &str) -> String {
    format!(
        "{API_ROOT}organizzazione/{}/farmaci/{}/stampati?ts={kind}",
        reference.codice_sis, reference.aic6
    )
}

fn aifa_request_error(error: ureq::Error) -> String {
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
    }
}

fn aifa_error_detail(body: &str) -> Option<String> {
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
}

fn write_temporary(path: &Path, content: &[u8]) -> Result<(), OperationError> {
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
}

fn write_download_dates(
    build_root: &Path,
    downloaded_at: &BTreeMap<String, String>,
) -> Result<(), OperationError> {
    let directory = build_root.join("manifests");
    fs::create_dir_all(&directory).map_err(|error| {
        aifa_persistence_error(format!(
            "creazione directory {}: {error}",
            directory.display()
        ))
    })?;
    let content = serde_json::to_vec_pretty(&serde_json::json!({"downloaded_at": downloaded_at}))
        .map_err(|error| {
        aifa_persistence_error(format!("serializzazione manifest: {error}"))
    })?;
    fs::write(directory.join(DOWNLOAD_MANIFEST), content).map_err(|error| {
        aifa_persistence_error(format!("scrittura manifest download AIFA: {error}"))
    })
}

/// Cerca candidati usando il client AIFA predefinito.
#[must_use]
pub fn lookup_aifa_candidate_search(
    medication: &Medication,
    query: Option<&str>,
) -> Option<AifaCandidateSearch> {
    AifaClient::new().lookup_candidate_search(medication, query)
}

/// Restituisce soltanto le corrispondenze esatte, distinguendo servizio
/// irraggiungibile (`None`) da una ricerca valida senza risultati.
#[must_use]
pub fn lookup_aifa_candidates(medication: &Medication) -> Option<Vec<AifaCandidate>> {
    Some(lookup_aifa_candidate_search(medication, None)?.exact)
}

/// Cerca le corrispondenze esatte; in caso di errore di rete restituisce una
/// lista vuota. Per distinguere l'errore da una ricerca senza risultati usare
/// [`lookup_aifa_candidates`].
#[must_use]
pub fn find_aifa_candidates(medication: &Medication) -> Vec<AifaCandidate> {
    lookup_aifa_candidates(medication).unwrap_or_default()
}

fn lookup_queries(medication: &Medication, query: Option<&str>) -> Vec<String> {
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
        let normalized = normalized_text(value);
        if !normalized.is_empty() && !seen.contains(&normalized) {
            seen.push(normalized);
            result.push(value.trim().to_owned());
        }
    }
    result
}

fn lookup_candidate_search_with<F>(
    medication: &Medication,
    query: Option<&str>,
    mut fetch_json: F,
) -> Option<AifaCandidateSearch>
where
    F: FnMut(&str) -> Option<Value>,
{
    let queries = lookup_queries(medication, query);
    if queries.is_empty() {
        return Some(AifaCandidateSearch::default());
    }

    let mut candidates = Vec::new();
    let mut available = false;
    for lookup_query in queries {
        let path = format!(
            "formadosaggio/ricerca?query={}&page=0",
            encode_query(&lookup_query)
        );
        let Some(payload) = fetch_json(&path) else {
            continue;
        };
        available = true;
        candidates.extend(candidates_from_payload(&payload));
    }
    if !available {
        return None;
    }

    let mut all_candidates = ordered_candidates(candidates);
    for candidate in &mut all_candidates {
        candidate.mismatches = candidate_mismatches(candidate, medication);
    }
    let exact = all_candidates
        .iter()
        .filter(|candidate| candidate.mismatches.is_empty())
        .cloned()
        .collect();
    let review = all_candidates
        .iter()
        .filter(|candidate| {
            !candidate.mismatches.is_empty() && candidate_is_plausible(candidate, medication)
        })
        .cloned()
        .collect();
    Some(AifaCandidateSearch {
        exact,
        review,
        all_candidates,
    })
}

fn candidates_from_payload(payload: &Value) -> Vec<AifaCandidate> {
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
                .and_then(json_string)?;
            let codice_sis = medicine.get("codiceSis").and_then(json_string)?;
            let aic6 = medicine.get("aic6").and_then(json_string)?;
            Some(AifaCandidate {
                title,
                codice_sis,
                aic6,
                active_ingredients: string_array(item.get("principiAttiviIt")),
                form: optional_string(item.get("formaFarmaceutica")),
                strength: optional_string(item.get("descrizioneFormaDosaggio")),
                company: optional_string(medicine.get("aziendaTitolare")),
                atc: string_array(item.get("descrizioneAtc")),
                routes: string_array(item.get("vieSomministrazione")),
                packages: item
                    .get("confezioni")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|package| optional_string(package.get("denominazionePackage")))
                    .collect(),
                mismatches: Vec::new(),
            })
        })
        .collect()
}

fn json_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::trim))
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn ordered_candidates(mut candidates: Vec<AifaCandidate>) -> Vec<AifaCandidate> {
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
}

fn candidate_mismatches(candidate: &AifaCandidate, medication: &Medication) -> Vec<String> {
    let mut mismatches = Vec::new();
    if medication
        .active_ingredient
        .as_ref()
        .is_some_and(|value| !value.is_empty())
        && !active_ingredients_match(candidate, medication)
    {
        mismatches.push("principio attivo".into());
    }
    if medication
        .form
        .as_ref()
        .is_some_and(|value| !value.is_empty())
        && !candidate.form.as_ref().is_some_and(|form| {
            !form_stems(form)
                .is_disjoint(&form_stems(medication.form.as_deref().unwrap_or_default()))
        })
    {
        mismatches.push("forma".into());
    }
    if medication
        .strength_per_unit
        .as_ref()
        .is_some_and(|value| !value.is_empty())
        && !candidate.strength.as_ref().is_some_and(|strength| {
            dose_signatures(medication.strength_per_unit.as_deref().unwrap_or_default())
                .is_subset(&dose_signatures(strength))
        })
    {
        mismatches.push("dosaggio".into());
    }
    mismatches
}

fn active_ingredients_match(candidate: &AifaCandidate, medication: &Medication) -> bool {
    let Some(active_ingredient) = medication.active_ingredient.as_deref() else {
        return false;
    };
    let components = active_ingredient_components(active_ingredient);
    let haystack = candidate
        .active_ingredients
        .iter()
        .map(|item| normalized_text(item))
        .collect::<Vec<_>>()
        .join(" ");
    !components.is_empty()
        && !haystack.is_empty()
        && components
            .iter()
            .all(|component| haystack.contains(component))
}

fn active_ingredient_components(value: &str) -> Vec<String> {
    static SEPARATOR: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    SEPARATOR
        .get_or_init(|| {
            Regex::new(r"(?i)\s*(?:,|\+|/|\be\b)\s*").expect("separatore principi attivi valido")
        })
        .split(value)
        .map(normalized_text)
        .filter(|item| !item.is_empty())
        .collect()
}

fn candidate_is_plausible(candidate: &AifaCandidate, medication: &Medication) -> bool {
    active_ingredients_match(candidate, medication)
        || commercial_names_match(&candidate.title, &medication.name)
}

fn commercial_names_match(candidate_name: &str, medication_name: &str) -> bool {
    let candidate = normalized_text(candidate_name);
    let medication = normalized_text(medication_name);
    candidate.len() >= 4
        && !medication.is_empty()
        && (candidate.contains(&medication) || medication.contains(&candidate))
}

fn normalized_text(value: &str) -> String {
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
}

fn form_stems(value: &str) -> BTreeSet<String> {
    normalized_text(value)
        .split_whitespace()
        .filter_map(|token| {
            let stem = token.trim_end_matches(['a', 'e', 'i', 'o', 'p']);
            let stem = stem.chars().take(8).collect::<String>();
            (stem.chars().count() >= 4).then_some(stem)
        })
        .collect()
}

fn dose_signatures(value: &str) -> BTreeSet<(String, String)> {
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
}

fn encode_query(value: &str) -> String {
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
}

#[cfg(test)]
mod tests;
