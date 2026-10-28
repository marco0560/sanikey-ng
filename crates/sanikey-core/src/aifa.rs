//! Ricerca dei candidati farmaco nel catalogo AIFA.

use crate::metadata::Medication;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::time::Duration;

const API_ROOT: &str = "https://api.aifa.gov.it/aifa-bdf-eif-be/1.0.0/";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
