//! Caricamento e validazione delle regole curate per i parametri clinici.

use crate::config::SearchDictionary;
use crate::metadata::{ObservationPoint, ObservationSeries};
use crate::operation::{ErrorCode, OperationError};
use crate::parameter_slices::{DiscoverySettings, ParameterCandidate, normalize_label};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use toml::{Table, Value};
use unicode_normalization::UnicodeNormalization;

const FORMATS: &[&str] = &[
    "integer",
    "decimal-comma",
    "decimal-point",
    "grouped-comma-decimal-point",
    "grouped-point-decimal-comma",
];
const UNIT_POLICIES: &[&str] = &["required", "allowed-but-unknown", "assume-configured-unit"];
const VALUE_TYPES: &[&str] = &["scalar", "qualified-scalar"];
const ROOT_FIELDS: &[&str] = &["discovery", "parameters"];
const DISCOVERY_FIELDS: &[&str] = &[
    "min_label_length",
    "max_label_length",
    "max_label_words",
    "min_occurrences",
    "min_distinct_documents",
    "min_distinct_dates",
    "excluded_labels",
];
const REQUIRED_RULE_FIELDS: &[&str] = &[
    "display_name",
    "term",
    "version",
    "value_type",
    "number_formats",
    "unit_policy",
    "enabled",
];
const OPTIONAL_RULE_FIELDS: &[&str] = &[
    "series_id",
    "units",
    "canonical_unit",
    "assumed_unit",
    "minimum",
    "maximum",
    "document_categories",
    "required_context",
    "excluded_context",
    "conversions",
    "specimen",
    "default_specimen_labels",
];

/// Conversione affine dichiarata esplicitamente da chi cura le regole.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitConversion {
    /// Unità accettata in ingresso.
    pub from_unit: String,
    /// Unità prodotta dalla conversione.
    pub to_unit: String,
    /// Moltiplicatore configurato.
    pub multiplier: Decimal,
    /// Termine additivo configurato.
    pub offset: Decimal,
    /// Versione dichiarata della conversione.
    pub version: i64,
}

/// Regola validata e risolta tramite il dizionario curato.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterRule {
    /// Identificatore stabile della regola.
    pub id: String,
    /// Nome mostrato all'utente.
    pub display_name: String,
    /// Etichette normalizzate riconosciute esattamente.
    pub synonyms: Vec<String>,
    /// Versione dichiarata della regola.
    pub version: i64,
    /// Tipo di valore ammesso.
    pub value_type: String,
    /// Formati numerici ammessi.
    pub number_formats: Vec<String>,
    /// Politica per unità mancanti o non note.
    pub unit_policy: String,
    /// Indica se la regola è attiva.
    pub enabled: bool,
    /// Serie canonica di destinazione.
    pub series_id: String,
    /// Unità ammesse.
    pub units: Vec<String>,
    /// Unità canonica facoltativa.
    pub canonical_unit: Option<String>,
    /// Unità da assumere solo con politica esplicita.
    pub assumed_unit: Option<String>,
    /// Limite minimo inclusivo.
    pub minimum: Option<Decimal>,
    /// Limite massimo inclusivo.
    pub maximum: Option<Decimal>,
    /// Categorie documento ammesse dopo normalizzazione.
    pub document_categories: Vec<String>,
    /// Frammenti richiesti sulla riga originale.
    pub required_context: Vec<String>,
    /// Frammenti che escludono la riga originale.
    pub excluded_context: Vec<String>,
    /// Conversioni esplicite ammesse.
    pub conversions: Vec<UnitConversion>,
    /// Campione biologico richiesto, quando specificato.
    pub specimen: Option<String>,
    /// Etichette che possono usare il campione predefinito.
    pub default_specimen_labels: Vec<String>,
    /// Digest SHA-256 del contratto risolto.
    pub digest: String,
}

/// Configurazione di scoperta e regole di un archivio.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterRules {
    /// Limiti per la scoperta di candidati.
    pub discovery: DiscoverySettings,
    /// Regole ordinate per identificatore.
    pub rules: Vec<ParameterRule>,
}

/// Esito esplicito della valutazione di una regola su un candidato.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleDecision {
    /// Identificatore stabile del candidato.
    pub candidate_id: String,
    /// Identificatore della regola valutata.
    pub rule_id: String,
    /// Indica se il candidato è accettato dalla regola.
    pub accepted: bool,
    /// Codice stabile che spiega l'esito.
    pub reason_code: String,
}

/// Serie, punti derivati e decisioni delle regole per un insieme di candidati.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParameterBuildResult {
    /// Serie create dalle regole che hanno accettato almeno un candidato.
    pub series: Vec<ObservationSeries>,
    /// Punti con provenienza costruiti dai candidati accettati.
    pub points: Vec<ObservationPoint>,
    /// Decisioni positive o motivi di rifiuto per ciascuna regola valutata.
    pub decisions: Vec<RuleDecision>,
}

/// Applica unità e conversioni configurate e materializza punti/serie.
///
/// Le conversioni e i controlli sui limiti usano numeri decimali esatti.
/// Decisioni ambigue, valori senza una rappresentazione `Decimal` e conversioni
/// fuori dal dominio decimale supportato sono rifiutati esplicitamente.
#[must_use]
pub fn build_parameter_slices(
    candidates: &[ParameterCandidate],
    rules: &ParameterRules,
) -> ParameterBuildResult {
    let mut result = ParameterBuildResult::default();
    let mut resolved = BTreeMap::new();
    let rules_by_id = rules
        .rules
        .iter()
        .map(|rule| (rule.id.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    let candidates_by_id = candidates
        .iter()
        .map(|candidate| (candidate.stable_id.as_str(), candidate))
        .collect::<BTreeMap<_, _>>();
    result.decisions = evaluate_parameter_candidates(candidates, &rules.rules);

    for decision in &mut result.decisions {
        if !decision.accepted {
            continue;
        }
        let Some(candidate) = candidates_by_id
            .get(decision.candidate_id.as_str())
            .copied()
        else {
            continue;
        };
        let rule = rules_by_id[decision.rule_id.as_str()];
        match value_and_unit(candidate, rule) {
            Ok((value, unit)) => {
                let series_id = if unit
                    .as_deref()
                    .is_none_or(|unit| rule.canonical_unit.as_deref() == Some(unit))
                {
                    rule.series_id.clone()
                } else {
                    format!(
                        "{}--{}",
                        rule.series_id,
                        unit_slug(unit.as_deref().unwrap_or(""))
                    )
                };
                resolved.insert(
                    decision.candidate_id.clone(),
                    (rule, candidate, value, unit, series_id),
                );
            }
            Err(reason) => {
                decision.accepted = false;
                decision.reason_code = reason.into();
            }
        }
    }

    for (rule, candidate, value, unit, series_id) in resolved.values() {
        let Some(date) = candidate.document_date.as_deref() else {
            continue;
        };
        let Some(numeric_value) = decimal_to_f64(*value) else {
            if let Some(decision) = result.decisions.iter_mut().find(|decision| {
                decision.candidate_id == candidate.stable_id && decision.rule_id == rule.id
            }) {
                decision.accepted = false;
                decision.reason_code = "REJECTED_FLOAT_RANGE".into();
            }
            continue;
        };
        let point_id = format!(
            "{:x}",
            Sha256::digest(format!("{}\u{1f}{}", rule.id, candidate.stable_id).as_bytes())
        );
        result.points.push(ObservationPoint {
            id: point_id,
            series_id: series_id.clone(),
            observation_date: date.into(),
            source_type: "document-extraction".into(),
            source_reference: candidate
                .document_href
                .clone()
                .unwrap_or_else(|| candidate.document_title.clone()),
            numeric_value: Some(numeric_value),
            text_value: None,
            systolic: None,
            diastolic: None,
            pulse: None,
            note: None,
            source_kind: "document-extraction".into(),
            document_id: Some(candidate.document_id.clone()),
            document_href: candidate.document_href.clone(),
            document_title: Some(candidate.document_title.clone()),
            document_category: Some(candidate.document_category.clone()),
            source_text_digest: Some(candidate.source_text_digest.clone()),
            original_line: Some(candidate.original_line.clone()),
            line_number: i64::try_from(candidate.line_number).ok(),
            page_number: candidate
                .page_number
                .and_then(|value| i64::try_from(value).ok()),
            character_start: i64::try_from(candidate.character_start).ok(),
            character_end: i64::try_from(candidate.character_end).ok(),
            matched_label: Some(candidate.normalized_label.clone()),
            raw_value: Some(candidate.raw_value.clone()),
            parsed_value: candidate.parsed_value,
            raw_unit: candidate.raw_unit.clone(),
            normalized_unit: unit.clone(),
            qualifier: candidate.qualifier.clone(),
            rule_id: Some(rule.id.clone()),
            rule_version: Some(rule.version),
            rule_digest: Some(rule.digest.clone()),
            reason_code: result
                .decisions
                .iter()
                .find(|decision| {
                    decision.candidate_id == candidate.stable_id && decision.rule_id == rule.id
                })
                .map(|decision| decision.reason_code.clone()),
        });
        resolved_series(&mut result.series, rule, unit.as_deref(), series_id);
    }
    result.points.sort_by(|left, right| {
        (&left.series_id, &left.observation_date, &left.id).cmp(&(
            &right.series_id,
            &right.observation_date,
            &right.id,
        ))
    });
    result.series.sort_by(|left, right| left.id.cmp(&right.id));
    result
}

/// Valuta candidati contro regole abilitate senza creare punti o applicare conversioni.
///
/// Per ogni regola abilitata emette un rifiuto al primo controllo fallito o
/// un'accettazione provvisoria. Se più regole accettano lo stesso candidato,
/// tutte le relative decisioni positive diventano rifiuti ambigui. Le decisioni
/// sono ordinate per candidato, regola e codice, indipendentemente dall'ordine
/// degli input.
#[must_use]
pub fn evaluate_parameter_candidates(
    candidates: &[ParameterCandidate],
    rules: &[ParameterRule],
) -> Vec<RuleDecision> {
    let mut decisions = Vec::new();
    for candidate in candidates {
        let mut matches = Vec::new();
        for rule in rules.iter().filter(|rule| rule.enabled) {
            if let Some(reason) = evaluate_rule(candidate, rule) {
                if reason.starts_with("ACCEPTED_") {
                    matches.push(rule.id.as_str());
                }
                decisions.push(RuleDecision {
                    candidate_id: candidate.stable_id.clone(),
                    rule_id: rule.id.clone(),
                    accepted: reason.starts_with("ACCEPTED_"),
                    reason_code: reason,
                });
            }
        }
        if matches.len() > 1 {
            for decision in decisions.iter_mut().filter(|decision| {
                decision.candidate_id == candidate.stable_id
                    && matches.contains(&decision.rule_id.as_str())
                    && decision.accepted
            }) {
                decision.accepted = false;
                decision.reason_code = "REJECTED_MULTIPLE_RULE_MATCHES".into();
            }
        }
    }
    decisions.sort_by(|left, right| {
        (&left.candidate_id, &left.rule_id, &left.reason_code).cmp(&(
            &right.candidate_id,
            &right.rule_id,
            &right.reason_code,
        ))
    });
    decisions
}

fn evaluate_rule(candidate: &ParameterCandidate, rule: &ParameterRule) -> Option<String> {
    let label = normalized_nfkc(&candidate.normalized_label);
    if !rule
        .synonyms
        .iter()
        .any(|synonym| normalized_nfkc(synonym) == label)
    {
        return Some("REJECTED_LABEL".into());
    }
    let mut accepted = "ACCEPTED_CONFIGURED_SYNONYM";
    if let Some(specimen) = rule.specimen.as_deref() {
        let explicit = if label.starts_with("s-") {
            Some("serum")
        } else if label.starts_with("u-") || label.contains("urinari") {
            Some("urine")
        } else {
            None
        };
        let observed = explicit.or(candidate.section_specimen.as_deref());
        if observed.is_some_and(|observed| observed != specimen) {
            return Some("REJECTED_SPECIMEN_MISMATCH".into());
        }
        if observed.is_none() {
            if !rule
                .default_specimen_labels
                .iter()
                .any(|item| normalized_nfkc(item) == label)
            {
                return Some("REJECTED_SPECIMEN_AMBIGUOUS".into());
            }
            accepted = "ACCEPTED_DEFAULT_SPECIMEN";
        }
    }
    if candidate.parsed_value.is_none()
        || !candidate
            .number_format
            .as_ref()
            .is_some_and(|format| rule.number_formats.contains(format))
    {
        return Some(
            candidate
                .reason_code
                .as_deref()
                .unwrap_or("REJECTED_NUMBER_FORMAT")
                .into(),
        );
    }
    if candidate.qualifier.is_some() && rule.value_type != "qualified-scalar" {
        return Some("REJECTED_QUALIFIER_UNSUPPORTED".into());
    }
    if candidate.document_date.is_none() {
        return Some("REJECTED_DOCUMENT_DATE_MISSING".into());
    }
    if !rule.document_categories.is_empty()
        && !rule.document_categories.iter().any(|category| {
            normalized_nfkc(category) == normalized_nfkc(&candidate.document_category)
        })
    {
        return Some("REJECTED_DOCUMENT_CATEGORY".into());
    }
    let line = normalized_nfkc(&candidate.original_line);
    if rule
        .required_context
        .iter()
        .any(|item| !line.contains(&normalized_nfkc(item)))
    {
        return Some("REJECTED_REQUIRED_CONTEXT_MISSING".into());
    }
    if rule
        .excluded_context
        .iter()
        .any(|item| line.contains(&normalized_nfkc(item)))
    {
        return Some("REJECTED_EXCLUDED_CONTEXT".into());
    }
    Some(accepted.into())
}

fn normalized_nfkc(value: &str) -> String {
    normalize_label(&value.nfkc().collect::<String>())
}

fn value_and_unit(
    candidate: &ParameterCandidate,
    rule: &ParameterRule,
) -> Result<(Decimal, Option<String>), &'static str> {
    let Some(format) = candidate.number_format.as_deref() else {
        return Err("REJECTED_NUMBER_FORMAT");
    };
    let Some(value) = parse_decimal(&candidate.raw_value, format) else {
        return Err("REJECTED_DECIMAL_PRECISION");
    };
    let unit = candidate.raw_unit.as_deref().map(normalize_unit);
    let Some(unit) = unit else {
        if rule.unit_policy == "required" {
            return Err("REJECTED_MISSING_UNIT");
        }
        return check_range(value, rule.assumed_unit.clone(), rule);
    };
    let configured_unit = rule
        .units
        .iter()
        .find(|configured| unit_key(configured) == unit_key(&unit));
    if !rule.units.is_empty() && configured_unit.is_none() {
        return Err("REJECTED_UNKNOWN_UNIT");
    }
    if let Some(conversion) = rule
        .conversions
        .iter()
        .find(|conversion| unit_key(&unit) == unit_key(&conversion.from_unit))
    {
        let converted = value
            .checked_mul(conversion.multiplier)
            .and_then(|value| value.checked_add(conversion.offset))
            .ok_or("REJECTED_CONVERSION_OVERFLOW")?;
        return check_range(converted, Some(conversion.to_unit.clone()), rule);
    }
    let output_unit = configured_unit.cloned().unwrap_or(unit);
    if rule
        .canonical_unit
        .as_deref()
        .is_some_and(|canonical| unit_key(&output_unit) != unit_key(canonical))
    {
        return Err("REJECTED_UNKNOWN_UNIT");
    }
    check_range(value, Some(output_unit), rule)
}

fn check_range(
    value: Decimal,
    unit: Option<String>,
    rule: &ParameterRule,
) -> Result<(Decimal, Option<String>), &'static str> {
    if rule.minimum.is_some_and(|minimum| value < minimum)
        || rule.maximum.is_some_and(|maximum| value > maximum)
    {
        return Err("REJECTED_OUT_OF_RANGE");
    }
    Ok((value, unit))
}

fn parse_decimal(raw: &str, format: &str) -> Option<Decimal> {
    let canonical = match format {
        "integer" => raw.to_owned(),
        "decimal-comma" => raw.replace(',', "."),
        "decimal-point" => raw.to_owned(),
        "grouped-comma-decimal-point" => raw.replace(',', ""),
        "grouped-point-decimal-comma" => raw.replace('.', "").replace(',', "."),
        _ => return None,
    };
    Decimal::from_str_exact(&canonical).ok()
}

fn unit_key(value: &str) -> String {
    normalize_unit(value).to_lowercase()
}

fn unit_slug(value: &str) -> String {
    let slug = value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "unknown".into()
    } else {
        slug.into()
    }
}

fn decimal_to_f64(value: Decimal) -> Option<f64> {
    let converted = value.to_string().parse::<f64>().ok()?;
    converted.is_finite().then_some(converted)
}

fn resolved_series(
    series: &mut Vec<ObservationSeries>,
    rule: &ParameterRule,
    unit: Option<&str>,
    id: &str,
) {
    if series.iter().any(|item| item.id == id) {
        return;
    }
    series.push(ObservationSeries {
        id: id.into(),
        name: rule.display_name.clone(),
        value_type: rule.value_type.clone(),
        unit: unit.map(str::to_owned),
        description: None,
        warn_duplicate_same_day: true,
        synonyms: rule.synonyms.clone(),
        parameter_rule_id: Some(rule.id.clone()),
        parameter_rule_version: Some(rule.version),
        parameter_rule_digest: Some(rule.digest.clone()),
        unit_variant: unit
            .filter(|unit| rule.canonical_unit.as_deref() != Some(*unit))
            .map(str::to_owned),
    });
}

/// Carica le regole TOML, risolvendo gli alias dal dizionario di ricerca.
///
/// Un file inesistente produce i default senza regole. I file presenti devono
/// avere una struttura valida e contenere solo campi conosciuti.
///
/// # Errors
///
/// Restituisce un errore di configurazione per errori di lettura, TOML invalido
/// o campi non validi.
pub fn load_parameter_rules(
    path: impl AsRef<Path>,
    dictionary: &SearchDictionary,
) -> Result<ParameterRules, OperationError> {
    let path = path.as_ref();
    let Ok(source) = fs::read_to_string(path) else {
        if !path.exists() {
            return Ok(ParameterRules {
                discovery: DiscoverySettings::default(),
                rules: Vec::new(),
            });
        }
        return Err(config_error(format!(
            "impossibile leggere regole parametri in {}",
            path.display()
        )));
    };
    let data: Value = toml::from_str(&source)
        .map_err(|cause| config_error(format!("TOML non valido in {}: {cause}", path.display())))?;
    parse_parameter_rules(&data, dictionary, path)
}

/// Carica le regole comuni e sostituisce per intero quelle ridefinite dal paziente.
///
/// Le impostazioni di scoperta del paziente prevalgono solo se la sua tabella
/// dichiara esplicitamente `[discovery]`.
///
/// # Errors
///
/// Restituisce un errore di configurazione se manca il file comune o se uno dei
/// file non è leggibile o valido.
pub fn load_patient_parameter_rules(
    common_path: impl AsRef<Path>,
    patient_path: impl AsRef<Path>,
    dictionary: &SearchDictionary,
) -> Result<ParameterRules, OperationError> {
    let common_path = common_path.as_ref();
    let patient_path = patient_path.as_ref();
    if !common_path.is_file() {
        return Err(config_error(format!(
            "regole parametri comuni assenti: {}",
            common_path.display()
        )));
    }
    let common = load_parameter_rules(common_path, dictionary)?;
    let patient = load_parameter_rules(patient_path, dictionary)?;
    let mut rules = common
        .rules
        .into_iter()
        .map(|rule| (rule.id.clone(), rule))
        .collect::<BTreeMap<_, _>>();
    rules.extend(
        patient
            .rules
            .into_iter()
            .map(|rule| (rule.id.clone(), rule)),
    );
    let discovery = if patient_path.is_file() && has_discovery(patient_path) {
        patient.discovery
    } else {
        common.discovery
    };
    Ok(ParameterRules {
        discovery,
        rules: rules.into_values().collect(),
    })
}

fn parse_parameter_rules(
    data: &Value,
    dictionary: &SearchDictionary,
    path: &Path,
) -> Result<ParameterRules, OperationError> {
    let root = data
        .as_table()
        .ok_or_else(|| config_error(format!("file parametri non valido: {}", path.display())))?;
    reject_unknown(
        root,
        ROOT_FIELDS,
        &format!("file parametri {}", path.display()),
    )?;
    let discovery = parse_discovery(root.get("discovery"), path)?;
    let raw_rules = match root.get("parameters") {
        Some(value) => value.as_table().ok_or_else(|| {
            config_error(format!(
                "{} parameters deve essere una tabella",
                path.display()
            ))
        })?,
        None => {
            return Ok(ParameterRules {
                discovery,
                rules: Vec::new(),
            });
        }
    };
    let mut rules = raw_rules
        .iter()
        .map(|(id, raw)| parse_rule(id, raw, dictionary, path))
        .collect::<Result<Vec<_>, _>>()?;
    rules.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(ParameterRules { discovery, rules })
}

fn has_discovery(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|source| toml::from_str::<Value>(&source).ok())
        .and_then(|value| {
            value
                .as_table()
                .map(|table| table.contains_key("discovery"))
        })
        .unwrap_or(false)
}

fn parse_discovery(
    value: Option<&Value>,
    path: &Path,
) -> Result<DiscoverySettings, OperationError> {
    let Some(value) = value else {
        return Ok(DiscoverySettings::default());
    };
    let table = value.as_table().ok_or_else(|| {
        config_error(format!(
            "{} discovery deve essere una tabella",
            path.display()
        ))
    })?;
    reject_unknown(
        table,
        DISCOVERY_FIELDS,
        &format!("{} discovery", path.display()),
    )?;
    let settings = DiscoverySettings {
        min_label_length: positive_usize(table, "min_label_length", Some(2), path)?,
        max_label_length: positive_usize(table, "max_label_length", Some(80), path)?,
        max_label_words: positive_usize(table, "max_label_words", Some(8), path)?,
        min_occurrences: positive_usize(table, "min_occurrences", Some(2), path)?,
        min_distinct_documents: positive_usize(table, "min_distinct_documents", Some(2), path)?,
        min_distinct_dates: positive_usize(table, "min_distinct_dates", Some(1), path)?,
        excluded_labels: strings(table.get("excluded_labels"), path, "excluded_labels")?,
    };
    if settings.max_label_length < settings.min_label_length {
        return Err(config_error(format!(
            "{} discovery max_label_length non puo' essere minore del minimo",
            path.display()
        )));
    }
    Ok(settings)
}

fn parse_rule(
    id: &str,
    value: &Value,
    dictionary: &SearchDictionary,
    path: &Path,
) -> Result<ParameterRule, OperationError> {
    let context = format!("{} parameters.{id}", path.display());
    let table = value
        .as_table()
        .ok_or_else(|| config_error(format!("{context} deve essere una tabella")))?;
    let allowed = REQUIRED_RULE_FIELDS
        .iter()
        .chain(OPTIONAL_RULE_FIELDS.iter())
        .copied()
        .collect::<Vec<_>>();
    reject_unknown(table, &allowed, &context)?;
    for field in REQUIRED_RULE_FIELDS {
        if !table.contains_key(*field) {
            return Err(config_error(format!(
                "{context} campo {field} obbligatorio"
            )));
        }
    }
    let term = string(table.get("term"), path, "term")?;
    let configured_synonyms = dictionary.terms.get(&term).ok_or_else(|| {
        config_error(format!(
            "{context} term non presente nel dizionario: {term}"
        ))
    })?;
    let mut synonyms = BTreeSet::from([normalize_label(&term)]);
    synonyms.extend(configured_synonyms.iter().map(|item| normalize_label(item)));
    let number_formats = strings(table.get("number_formats"), path, "number_formats")?;
    if number_formats.is_empty()
        || number_formats
            .iter()
            .any(|item| !FORMATS.contains(&item.as_str()))
    {
        return Err(config_error(format!("{context} number_formats non validi")));
    }
    let unit_policy = string(table.get("unit_policy"), path, "unit_policy")?;
    if !UNIT_POLICIES.contains(&unit_policy.as_str()) {
        return Err(config_error(format!(
            "{context} unit_policy non supportata"
        )));
    }
    let value_type = string(table.get("value_type"), path, "value_type")?;
    if !VALUE_TYPES.contains(&value_type.as_str()) {
        return Err(config_error(format!("{context} value_type non supportato")));
    }
    let enabled = table
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| config_error(format!("{context} enabled deve essere booleano")))?;
    let assumed_unit = optional_string(table.get("assumed_unit"), path, "assumed_unit")?
        .map(|item| normalize_unit(&item));
    if unit_policy == "assume-configured-unit" && assumed_unit.is_none() {
        return Err(config_error(format!("{context} richiede assumed_unit")));
    }
    let minimum = decimal(table.get("minimum"), path, "minimum")?;
    let maximum = decimal(table.get("maximum"), path, "maximum")?;
    if minimum
        .zip(maximum)
        .is_some_and(|(minimum, maximum)| minimum > maximum)
    {
        return Err(config_error(format!("{context} minimum supera maximum")));
    }
    let mut formats = number_formats;
    formats.sort();
    let mut units = strings(table.get("units"), path, "units")?
        .into_iter()
        .map(|item| normalize_unit(&item))
        .collect::<Vec<_>>();
    units.sort();
    let canonical_unit = optional_string(table.get("canonical_unit"), path, "canonical_unit")?
        .map(|item| normalize_unit(&item));
    let mut document_categories = normalized_strings(
        table.get("document_categories"),
        path,
        "document_categories",
    )?;
    let mut required_context =
        normalized_strings(table.get("required_context"), path, "required_context")?;
    let mut excluded_context =
        normalized_strings(table.get("excluded_context"), path, "excluded_context")?;
    let conversions = parse_conversions(table.get("conversions"), path, id)?;
    let specimen = optional_string(table.get("specimen"), path, "specimen")?;
    let mut default_specimen_labels = normalized_strings(
        table.get("default_specimen_labels"),
        path,
        "default_specimen_labels",
    )?;
    for values in [
        &mut document_categories,
        &mut required_context,
        &mut excluded_context,
        &mut default_specimen_labels,
    ] {
        values.sort();
    }
    let rule = ParameterRule {
        id: id.to_owned(),
        display_name: string(table.get("display_name"), path, "display_name")?,
        synonyms: synonyms.into_iter().collect(),
        version: positive(table, "version", None, path)?,
        value_type,
        number_formats: formats,
        unit_policy,
        enabled,
        series_id: optional_string(table.get("series_id"), path, "series_id")?
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| id.to_owned()),
        units,
        canonical_unit,
        assumed_unit,
        minimum,
        maximum,
        document_categories,
        required_context,
        excluded_context,
        conversions,
        specimen,
        default_specimen_labels,
        digest: String::new(),
    };
    let mut rule = rule;
    rule.digest = digest_rule(&rule);
    Ok(rule)
}

fn parse_conversions(
    value: Option<&Value>,
    path: &Path,
    id: &str,
) -> Result<Vec<UnitConversion>, OperationError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let entries = value.as_array().ok_or_else(|| {
        config_error(format!(
            "{} parameters.{id} conversions deve essere un array",
            path.display()
        ))
    })?;
    let mut conversions = Vec::with_capacity(entries.len());
    for item in entries {
        let context = format!("{} parameters.{id} conversione", path.display());
        let table = item
            .as_table()
            .ok_or_else(|| config_error(format!("{context} non valida")))?;
        reject_unknown(
            table,
            &["from_unit", "to_unit", "multiplier", "offset", "version"],
            &context,
        )?;
        conversions.push(UnitConversion {
            from_unit: normalize_unit(&string(table.get("from_unit"), path, "from_unit")?),
            to_unit: normalize_unit(&string(table.get("to_unit"), path, "to_unit")?),
            multiplier: decimal(table.get("multiplier"), path, "multiplier")?
                .unwrap_or(Decimal::ZERO),
            offset: decimal(table.get("offset"), path, "offset")?.unwrap_or(Decimal::ZERO),
            version: positive(table, "version", None, path)?,
        });
    }
    conversions.sort_by(|left, right| {
        (&left.from_unit, &left.to_unit, left.version).cmp(&(
            &right.from_unit,
            &right.to_unit,
            right.version,
        ))
    });
    Ok(conversions)
}

fn reject_unknown(table: &Table, allowed: &[&str], context: &str) -> Result<(), OperationError> {
    let mut unknown = table
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(config_error(format!(
            "{context} contiene campi sconosciuti: {}",
            unknown.join(", ")
        )))
    }
}

fn positive(
    table: &Table,
    key: &str,
    default: Option<i64>,
    path: &Path,
) -> Result<i64, OperationError> {
    let value = match table.get(key) {
        Some(value) => value.as_integer(),
        None => default,
    };
    value.filter(|value| *value > 0).ok_or_else(|| {
        config_error(format!(
            "{} campo {key} deve essere un intero positivo",
            path.display()
        ))
    })
}

fn positive_usize(
    table: &Table,
    key: &str,
    default: Option<i64>,
    path: &Path,
) -> Result<usize, OperationError> {
    usize::try_from(positive(table, key, default, path)?).map_err(|_| {
        config_error(format!(
            "{} campo {key} supera il limite supportato",
            path.display()
        ))
    })
}

fn string(value: Option<&Value>, path: &Path, key: &str) -> Result<String, OperationError> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            config_error(format!(
                "{} campo {key} deve essere una stringa non vuota",
                path.display()
            ))
        })
}

fn optional_string(
    value: Option<&Value>,
    path: &Path,
    key: &str,
) -> Result<Option<String>, OperationError> {
    value.map(|_| string(value, path, key)).transpose()
}

fn strings(value: Option<&Value>, path: &Path, key: &str) -> Result<Vec<String>, OperationError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| {
            config_error(format!(
                "{} campo {key} deve essere un array di stringhe",
                path.display()
            ))
        })?
        .iter()
        .map(|item| string(Some(item), path, key))
        .collect()
}

fn normalized_strings(
    value: Option<&Value>,
    path: &Path,
    key: &str,
) -> Result<Vec<String>, OperationError> {
    Ok(strings(value, path, key)?
        .into_iter()
        .map(|item| normalize_label(&item))
        .collect())
}

fn decimal(
    value: Option<&Value>,
    path: &Path,
    key: &str,
) -> Result<Option<Decimal>, OperationError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let number = if let Some(integer) = value.as_integer() {
        Decimal::from(integer)
    } else if let Some(float) = value.as_float().filter(|number| number.is_finite()) {
        let text = float.to_string();
        let parsed = if text.contains('e') || text.contains('E') {
            Decimal::from_scientific(&text)
        } else {
            Decimal::from_str_exact(&text)
        };
        parsed.map_err(|_| {
            config_error(format!(
                "{} campo {key} supera la precisione decimale supportata",
                path.display()
            ))
        })?
    } else {
        return Err(config_error(format!(
            "{} campo {key} deve essere numerico e finito",
            path.display()
        )));
    };
    Ok(Some(number))
}

fn normalize_unit(value: &str) -> String {
    value
        .nfkc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn digest_rule(rule: &ParameterRule) -> String {
    let conversions = rule
        .conversions
        .iter()
        .map(|item| {
            serde_json::json!([
                item.from_unit,
                item.to_unit,
                canonical_number(item.multiplier),
                canonical_number(item.offset),
                item.version
            ])
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "id": rule.id,
        "display_name": rule.display_name,
        "synonyms": rule.synonyms,
        "version": rule.version,
        "value_type": rule.value_type,
        "number_formats": rule.number_formats,
        "unit_policy": rule.unit_policy,
        "enabled": rule.enabled,
        "series_id": rule.series_id,
        "units": rule.units,
        "canonical_unit": rule.canonical_unit,
        "assumed_unit": rule.assumed_unit,
        "minimum": rule.minimum.map(canonical_number),
        "maximum": rule.maximum.map(canonical_number),
        "document_categories": rule.document_categories,
        "required_context": rule.required_context,
        "excluded_context": rule.excluded_context,
        "conversions": conversions,
        "specimen": rule.specimen,
        "default_specimen_labels": rule.default_specimen_labels
    });
    let canonical = serde_json::to_vec(&value).expect("regola serializzabile in JSON");
    format!("{:x}", Sha256::digest(canonical))
}

fn canonical_number(value: Decimal) -> String {
    value.to_string()
}

fn config_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}

#[cfg(test)]
mod tests;
