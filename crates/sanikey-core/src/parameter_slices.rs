//! Scoperta deterministica di candidati a parametri clinici nei testi estratti.

use crate::logging::{
    Component, TraceField, TraceValue, redact_secret_assignments, trace_current,
    trace_sensitive_text,
};
use crate::metadata::DocumentRecord;
use crate::pdf::{ExtractedText, TextPageSpan};
use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

macro_rules! trace_parameter_call {
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], fields [$($field_name:literal => $field_value:expr),* $(,)?], $body:expr) => {{
        let trace = trace_current(Component::Core, $function, || {
            Vec::from([
                $(TraceField::new($name, trace_debug($value)),)*
                $(TraceField::new($field_name, $field_value),)*
            ])
        });
        let result = run_parameter_body($body);
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!({
                "format":"rust_debug",
                "value":redact_secret_assignments(&format!("{result:#?}")),
            })));
        }
        result
    }};
    ($function:literal, [$($name:literal => $value:expr),* $(,)?], $body:expr) => {
        trace_parameter_call!($function, [$($name => $value),*], fields [], $body)
    };
}

fn run_parameter_body<R>(body: impl FnOnce() -> R) -> R {
    body()
}

fn trace_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> TraceValue {
    TraceValue::Data(json!({
        "format":"rust_debug",
        "value":redact_secret_assignments(&format!("{value:#?}")),
    }))
}

fn trace_document(document: &DocumentRecord) -> TraceValue {
    TraceValue::Data(json!({
        "format":"rust_debug",
        "value":redact_secret_assignments(&format!("{document:#?}")),
        "native_path":TraceValue::path(&document.path).to_json(),
    }))
}

fn trace_documents(documents: &[DocumentRecord]) -> TraceValue {
    TraceValue::Data(Value::Array(
        documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                json!({
                    "index":index,
                    "document":trace_document(document).to_json(),
                })
            })
            .collect(),
    ))
}

fn trace_extractions(extractions: &[ExtractedText]) -> TraceValue {
    TraceValue::Data(Value::Array(
        extractions
            .iter()
            .enumerate()
            .map(|(index, extraction)| json!({
                "index":index,
                "document_id":redact_secret_assignments(&extraction.document_id),
                "text":trace_sensitive_text(&extraction.text).to_json(),
                "warnings":extraction.warnings.iter().map(|warning| redact_secret_assignments(warning)).collect::<Vec<_>>(),
                "page_spans":extraction.page_spans.iter().map(|span| json!({
                    "page_number":span.page_number,
                    "character_start":span.character_start,
                    "character_end":span.character_end,
                })).collect::<Vec<_>>(),
            }))
            .collect(),
    ))
}

fn trace_optional_hrefs(hrefs: Option<&HashMap<String, String>>) -> TraceValue {
    TraceValue::Data(hrefs.map_or(Value::Null, |hrefs| {
        let ordered = hrefs.iter().collect::<BTreeMap<_, _>>();
        Value::Object(
            ordered
                .into_iter()
                .map(|(key, value)| {
                    (
                        redact_secret_assignments(key),
                        json!(redact_secret_assignments(value)),
                    )
                })
                .collect(),
        )
    }))
}

fn trace_optional_label_set(labels: Option<&HashSet<String>>) -> TraceValue {
    TraceValue::Data(labels.map_or(Value::Null, |labels| {
        json!(
            labels
                .iter()
                .map(|label| redact_secret_assignments(label))
                .collect::<BTreeSet<_>>()
        )
    }))
}

fn trace_lines(lines: &[&str]) -> TraceValue {
    TraceValue::Data(json!(
        lines
            .iter()
            .map(|line| redact_secret_assignments(line))
            .collect::<Vec<_>>()
    ))
}

fn trace_optional_text(value: Option<&str>) -> TraceValue {
    value.map_or(TraceValue::Data(Value::Null), trace_sensitive_text)
}

fn trace_bytes(bytes: &[u8]) -> TraceValue {
    match std::str::from_utf8(bytes) {
        Ok(text) => trace_sensitive_text(text),
        Err(_) => TraceValue::Data(json!({"encoding":"bytes","value":bytes})),
    }
}

/// Limiti per l'individuazione e la proposta di candidati.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoverySettings {
    /// Lunghezza minima dell'etichetta normalizzata.
    pub min_label_length: usize,
    /// Lunghezza massima dell'etichetta normalizzata.
    pub max_label_length: usize,
    /// Numero massimo di parole dell'etichetta.
    pub max_label_words: usize,
    /// Occorrenze minime per proporre un gruppo.
    pub min_occurrences: usize,
    /// Documenti distinti minimi per proporre un gruppo.
    pub min_distinct_documents: usize,
    /// Date distinte minime per proporre un gruppo.
    pub min_distinct_dates: usize,
    /// Etichette escluse dopo la normalizzazione.
    pub excluded_labels: Vec<String>,
}

impl Default for DiscoverySettings {
    fn default() -> Self {
        Self {
            min_label_length: 2,
            max_label_length: 80,
            max_label_words: 8,
            min_occurrences: 2,
            min_distinct_documents: 2,
            min_distinct_dates: 1,
            excluded_labels: Vec::new(),
        }
    }
}

/// Occorrenza sintattica con provenienza nel documento sorgente.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterCandidate {
    /// Identificatore SHA-256 stabile.
    pub stable_id: String,
    /// Identificatore del documento.
    pub document_id: String,
    /// Data autorevole del documento.
    pub document_date: Option<String>,
    /// Link relativo all'originale esportato.
    pub document_href: Option<String>,
    /// Titolo e categoria della fonte.
    pub document_title: String,
    /// Nome originale del documento.
    pub document_name: String,
    /// Categoria del documento.
    pub document_category: String,
    /// Digest del testo estratto.
    pub source_text_digest: String,
    /// Riga a partire da uno.
    pub line_number: usize,
    /// Offset Unicode iniziale inclusivo.
    pub character_start: usize,
    /// Offset Unicode finale esclusivo.
    pub character_end: usize,
    /// Riga sorgente non modificata.
    pub original_line: String,
    /// Etichetta normalizzata.
    pub normalized_label: String,
    /// Token numerico originale.
    pub raw_value: String,
    /// Valore numerico; resta assente se ambiguo o invalido.
    pub parsed_value: Option<f64>,
    /// Formato numerico riconosciuto.
    pub number_format: Option<String>,
    /// Qualificatore di confronto.
    pub qualifier: Option<String>,
    /// Unità originale.
    pub raw_unit: Option<String>,
    /// Testo prima e dopo il candidato nella riga.
    pub prefix_context: String,
    /// Testo dopo il candidato nella riga.
    pub suffix_context: String,
    /// Codice di rifiuto deterministico.
    pub reason_code: Option<String>,
    /// Pagina PDF a partire da uno, quando nota.
    pub page_number: Option<usize>,
    /// Riga candidata e righe adiacenti.
    pub context: String,
    /// Etichetta di pagina.
    pub page_label: String,
    /// Sezione esplicita `serum` o `urine`.
    pub section_specimen: Option<String>,
}

/// Candidati con un'etichetta normalizzata identica.
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateGroup {
    /// Etichetta condivisa.
    pub normalized_label: String,
    /// Candidati in ordine deterministico.
    pub candidates: Vec<ParameterCandidate>,
}

impl CandidateGroup {
    /// Conta i documenti sorgente distinti.
    #[must_use]
    pub fn distinct_documents(&self) -> usize {
        trace_parameter_call!("parameter_slices::CandidateGroup::distinct_documents", [], fields ["group" => trace_debug(self)], || {
        self.candidates
            .iter()
            .map(|item| item.document_id.as_str())
            .collect::<HashSet<_>>()
            .len()
        })
    }

    /// Conta le date autorevoli distinte.
    #[must_use]
    pub fn distinct_dates(&self) -> usize {
        trace_parameter_call!("parameter_slices::CandidateGroup::distinct_dates", [], fields ["group" => trace_debug(self)], || {
        self.candidates
            .iter()
            .filter_map(|item| item.document_date.as_deref())
            .collect::<HashSet<_>>()
            .len()
        })
    }
}

/// Candidati trovati e gruppi che superano le soglie di proposta.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiscoveryResult {
    /// Candidati ordinati.
    pub candidates: Vec<ParameterCandidate>,
    /// Gruppi idonei a una revisione della curatrice.
    pub proposed_groups: Vec<CandidateGroup>,
    /// Righe analizzate.
    pub analyzed_lines: usize,
}

/// Cerca candidati nelle righe già estratte senza inferire nuove misure.
///
/// Gli offset sono conteggiati in valori Unicode e riferiti al testo concatenato.
///
/// # Errors
///
/// Restituisce un errore quando i limiti non sono positivi o coerenti.
pub fn discover_candidates(
    documents: &[DocumentRecord],
    extracted_text: &[ExtractedText],
    document_hrefs: Option<&HashMap<String, String>>,
    settings: &DiscoverySettings,
    accepted_labels: Option<&HashSet<String>>,
) -> Result<DiscoveryResult, String> {
    trace_parameter_call!(
        "parameter_slices::discover_candidates",
        [],
        fields [
            "documents" => trace_documents(documents),
            "extracted_text" => trace_extractions(extracted_text),
            "document_hrefs" => trace_optional_hrefs(document_hrefs),
            "settings" => trace_debug(settings),
            "accepted_labels" => trace_optional_label_set(accepted_labels),
        ],
        || {
    validate_settings(settings)?;
    let records: HashMap<_, _> = documents
        .iter()
        .map(|item| (item.document_id.as_str(), item))
        .collect();
    let mut extractions = extracted_text.iter().collect::<Vec<_>>();
    extractions.sort_by(|left, right| left.document_id.cmp(&right.document_id));
    let mut candidates = Vec::new();
    let mut analyzed_lines = 0;
    for extracted in extractions {
        let Some(document) = records.get(extracted.document_id.as_str()) else {
            continue;
        };
        let digest = sha256(extracted.text.as_bytes());
        let lines = extracted.text.lines().collect::<Vec<_>>();
        let mut offsets = Vec::with_capacity(lines.len());
        let mut offset = 0;
        for line in &lines {
            offsets.push(offset);
            offset += line.chars().count() + 1;
        }
        for (index, line) in lines.iter().enumerate() {
            analyzed_lines += 1;
            let Some(mut candidate) = candidate_from_line(
                document,
                document_hrefs.and_then(|hrefs| hrefs.get(&document.document_id)),
                &digest,
                line,
                index + 1,
                offsets[index],
                settings,
            )
            .or_else(|| {
                candidate_from_stacked_cells(
                    document,
                    document_hrefs.and_then(|hrefs| hrefs.get(&document.document_id)),
                    &digest,
                    &lines,
                    index,
                    &offsets,
                    settings,
                )
            })
            .or_else(|| {
                candidate_from_reference_table_cells(
                    document,
                    document_hrefs.and_then(|hrefs| hrefs.get(&document.document_id)),
                    &digest,
                    &lines,
                    index,
                    &offsets,
                    settings,
                )
            })
            .or_else(|| {
                candidate_from_annotated_result(
                    document,
                    document_hrefs.and_then(|hrefs| hrefs.get(&document.document_id)),
                    &digest,
                    &lines,
                    index,
                    &offsets,
                    settings,
                )
            })
            .or_else(|| {
                candidate_from_annotated_value_description(
                    document,
                    document_hrefs.and_then(|hrefs| hrefs.get(&document.document_id)),
                    &digest,
                    &lines,
                    index,
                    &offsets,
                    settings,
                )
            }) else {
                continue;
            };
            if accepted_labels.is_some_and(|labels| !labels.contains(&candidate.normalized_label)) {
                continue;
            }
            candidate.page_number = page_number(&extracted.page_spans, candidate.character_start);
            candidate.page_label = candidate.page_number.map_or_else(
                || {
                    if document
                        .path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
                    {
                        "non disponibile".into()
                    } else {
                        "non applicabile".into()
                    }
                },
                |page| page.to_string(),
            );
            candidate.context = line_context(&lines, index);
            candidate.section_specimen = section_specimen(&lines, index);
            candidates.push(candidate);
        }
    }
    candidates.sort_by(|left, right| {
        (
            &left.normalized_label,
            left.document_date.as_deref().unwrap_or(""),
            &left.document_id,
            left.character_start,
            &left.stable_id,
        )
            .cmp(&(
                &right.normalized_label,
                right.document_date.as_deref().unwrap_or(""),
                &right.document_id,
                right.character_start,
                &right.stable_id,
            ))
    });
    let proposed_groups = proposed_groups(&candidates, settings);
    Ok(DiscoveryResult {
        candidates,
        proposed_groups,
        analyzed_lines,
    })
        }
    )
}

/// Cerca solo etichette configurate; i gruppi di proposta restano vuoti.
///
/// # Errors
///
/// Restituisce un errore quando i limiti non sono positivi o coerenti.
pub fn discover_configured_candidates(
    documents: &[DocumentRecord],
    extracted_text: &[ExtractedText],
    accepted_labels: &[String],
    document_hrefs: Option<&HashMap<String, String>>,
    settings: &DiscoverySettings,
) -> Result<DiscoveryResult, String> {
    trace_parameter_call!(
        "parameter_slices::discover_configured_candidates",
        [],
        fields [
            "documents" => trace_documents(documents),
            "extracted_text" => trace_extractions(extracted_text),
            "accepted_labels" => TraceValue::Data(json!(accepted_labels.iter().map(|label| redact_secret_assignments(label)).collect::<Vec<_>>())),
            "document_hrefs" => trace_optional_hrefs(document_hrefs),
            "settings" => trace_debug(settings),
        ],
        || {
    let accepted = accepted_labels
        .iter()
        .map(|item| normalize_label(item))
        .collect();
    let result = discover_candidates(
        documents,
        extracted_text,
        document_hrefs,
        settings,
        Some(&accepted),
    )?;
    Ok(DiscoveryResult {
        proposed_groups: Vec::new(),
        ..result
    })
        }
    )
}

/// Normalizza l'etichetta comprimendo gli spazi e ignorando maiuscole/minuscole.
#[must_use]
pub fn normalize_label(value: &str) -> String {
    trace_parameter_call!("parameter_slices::normalize_label", [], fields ["value" => trace_sensitive_text(value)], || {
    trailing_separator()
        .replace(value, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
    })
}

fn candidate_from_line(
    document: &DocumentRecord,
    href: Option<&String>,
    digest: &str,
    line: &str,
    line_number: usize,
    line_offset: usize,
    settings: &DiscoverySettings,
) -> Option<ParameterCandidate> {
    trace_parameter_call!(
        "parameter_slices::candidate_from_line",
        ["digest" => &digest, "line_number" => &line_number, "line_offset" => &line_offset],
        fields [
            "document" => trace_document(document),
            "href" => trace_optional_text(href.map(|value| value.as_str())),
            "line" => trace_sensitive_text(line),
            "settings" => trace_debug(settings),
        ],
        || {
    let captures = candidate_pattern().captures(line)?;
    let full = captures.get(0)?;
    let label = captures.name("label")?.as_str();
    let normalized_label = normalize_label(label);
    if !label_is_eligible(&normalized_label, settings) {
        return None;
    }
    let raw_value = captures.name("number")?.as_str();
    let (parsed_value, number_format, reason_code) = parse_number(raw_value);
    let start = line_offset + line[..full.start()].chars().count();
    let end = line_offset + line[..full.end()].chars().count();
    let raw_unit = captures.name("unit").map(|unit| unit.as_str().to_owned());
    let stable_id = sha256(
        [
            document.document_id.as_str(),
            digest,
            &start.to_string(),
            &end.to_string(),
            normalized_label.as_str(),
            raw_value,
            raw_unit.as_deref().unwrap_or(""),
        ]
        .join("\u{1f}")
        .as_bytes(),
    );
    Some(ParameterCandidate {
        stable_id,
        document_id: document.document_id.clone(),
        document_date: document.date.clone(),
        document_href: href.cloned(),
        document_title: document.title.clone(),
        document_name: document
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        document_category: document.category.clone(),
        source_text_digest: digest.to_owned(),
        line_number,
        character_start: start,
        character_end: end,
        original_line: line.to_owned(),
        normalized_label,
        raw_value: raw_value.to_owned(),
        parsed_value,
        number_format,
        qualifier: captures
            .name("qualifier")
            .map(|item| item.as_str().to_owned()),
        raw_unit,
        prefix_context: line[..full.start()].to_owned(),
        suffix_context: line[full.end()..].to_owned(),
        reason_code,
        page_number: None,
        context: String::new(),
        page_label: "non applicabile".into(),
        section_specimen: None,
    })
        }
    )
}

fn candidate_from_stacked_cells(
    document: &DocumentRecord,
    href: Option<&String>,
    digest: &str,
    lines: &[&str],
    label_index: usize,
    offsets: &[usize],
    settings: &DiscoverySettings,
) -> Option<ParameterCandidate> {
    trace_parameter_call!(
        "parameter_slices::candidate_from_stacked_cells",
        ["label_index" => &label_index],
        fields [
            "document" => trace_document(document),
            "href" => trace_optional_text(href.map(|value| value.as_str())),
            "digest" => trace_sensitive_text(digest),
            "lines" => trace_lines(lines),
            "offsets" => trace_debug(offsets),
            "settings" => trace_debug(settings),
        ],
        || {
    let cells = following_table_cells(lines, label_index, 2);
    let [(_, value_line), (unit_index, unit_line)] = cells.as_slice() else {
        return None;
    };
    let value = stacked_value_pattern().captures(value_line)?;
    let unit = stacked_unit_pattern().captures(unit_line)?;
    let raw_unit = unit_without_reference_range(unit.name("unit")?.as_str());
    if raw_unit.is_empty() {
        return None;
    }
    let label = lines[label_index];
    let normalized_label = normalize_label(label);
    if !label_is_eligible(&normalized_label, settings) {
        return None;
    }
    let raw_value = value.name("number")?.as_str();
    let candidate = candidate_from_line(
        document,
        href,
        digest,
        &format!(
            "{label} {} {raw_value} {raw_unit}",
            value.name("qualifier").map_or("", |m| m.as_str())
        ),
        label_index + 1,
        offsets[label_index],
        settings,
    )?;
    let unit_start = unit_line.find(&raw_unit)?;
    let end =
        offsets[*unit_index] + unit_line[..unit_start].chars().count() + raw_unit.chars().count();
    Some(finish_table_candidate(
        candidate,
        offsets[label_index],
        end,
        lines[label_index..=*unit_index].join("\n"),
        raw_value,
        &raw_unit,
    ))
        }
    )
}

fn candidate_from_reference_table_cells(
    document: &DocumentRecord,
    href: Option<&String>,
    digest: &str,
    lines: &[&str],
    label_index: usize,
    offsets: &[usize],
    settings: &DiscoverySettings,
) -> Option<ParameterCandidate> {
    trace_parameter_call!(
        "parameter_slices::candidate_from_reference_table_cells",
        ["label_index" => &label_index],
        fields [
            "document" => trace_document(document),
            "href" => trace_optional_text(href.map(|value| value.as_str())),
            "digest" => trace_sensitive_text(digest),
            "lines" => trace_lines(lines),
            "offsets" => trace_debug(offsets),
            "settings" => trace_debug(settings),
        ],
        || {
    let cells = following_table_cells(lines, label_index, 3);
    let [
        (_unit_index, unit_line),
        (_, reference_line),
        (value_index, value_line),
    ] = cells.as_slice()
    else {
        return None;
    };
    if !reference_cell_pattern().is_match(reference_line) {
        return None;
    }
    let unit = stacked_unit_pattern().captures(unit_line)?;
    let value = stacked_value_pattern().captures(value_line)?;
    let raw_unit = unit_without_reference_range(unit.name("unit")?.as_str());
    if raw_unit.is_empty() {
        return None;
    }
    let label = lines[label_index];
    let normalized_label = normalize_label(label);
    if !label_is_eligible(&normalized_label, settings) {
        return None;
    }
    let raw_value = value.name("number")?.as_str();
    let candidate = candidate_from_line(
        document,
        href,
        digest,
        &format!(
            "{label} {} {raw_value} {raw_unit}",
            value.name("qualifier").map_or("", |m| m.as_str())
        ),
        label_index + 1,
        offsets[label_index],
        settings,
    )?;
    let value_start = value_line.rfind(raw_value)?;
    let end = offsets[*value_index]
        + value_line[..value_start].chars().count()
        + raw_value.chars().count();
    Some(finish_table_candidate(
        candidate,
        offsets[label_index],
        end,
        lines[label_index..=*value_index].join("\n"),
        raw_value,
        &raw_unit,
    ))
        }
    )
}

fn candidate_from_annotated_result(
    document: &DocumentRecord,
    href: Option<&String>,
    digest: &str,
    lines: &[&str],
    label_index: usize,
    offsets: &[usize],
    settings: &DiscoverySettings,
) -> Option<ParameterCandidate> {
    trace_parameter_call!(
        "parameter_slices::candidate_from_annotated_result",
        ["label_index" => &label_index],
        fields [
            "document" => trace_document(document),
            "href" => trace_optional_text(href.map(|value| value.as_str())),
            "digest" => trace_sensitive_text(digest),
            "lines" => trace_lines(lines),
            "offsets" => trace_debug(offsets),
            "settings" => trace_debug(settings),
        ],
        || {
    let cells = following_table_cells(lines, label_index, 1);
    let [(value_index, value_line)] = cells.as_slice() else {
        return None;
    };
    let content = inline_result_pattern()
        .captures(value_line)?
        .name("content")?
        .as_str();
    let label = lines[label_index];
    let candidate = candidate_from_line(
        document,
        href,
        digest,
        &format!("{label} {content}"),
        label_index + 1,
        offsets[label_index],
        settings,
    )?;
    let consumed = candidate.character_end - offsets[label_index] - label.chars().count() - 1;
    let content_start = value_line.find(content)?;
    let end = offsets[*value_index] + value_line[..content_start].chars().count() + consumed;
    let raw_value = candidate.raw_value.clone();
    let raw_unit = candidate.raw_unit.clone().unwrap_or_default();
    Some(finish_table_candidate(
        candidate,
        offsets[label_index],
        end,
        lines[label_index..=*value_index].join("\n"),
        &raw_value,
        &raw_unit,
    ))
        }
    )
}

fn candidate_from_annotated_value_description(
    document: &DocumentRecord,
    href: Option<&String>,
    digest: &str,
    lines: &[&str],
    label_index: usize,
    offsets: &[usize],
    settings: &DiscoverySettings,
) -> Option<ParameterCandidate> {
    trace_parameter_call!(
        "parameter_slices::candidate_from_annotated_value_description",
        ["label_index" => &label_index],
        fields [
            "document" => trace_document(document),
            "href" => trace_optional_text(href.map(|value| value.as_str())),
            "digest" => trace_sensitive_text(digest),
            "lines" => trace_lines(lines),
            "offsets" => trace_debug(offsets),
            "settings" => trace_debug(settings),
        ],
        || {
    let label = lines[label_index];
    let normalized_label = normalize_label(label);
    if !label_is_eligible(&normalized_label, settings)
        || !glycated_hemoglobin_label_pattern().is_match(label)
    {
        return None;
    }
    let following = lines
        .iter()
        .enumerate()
        .skip(label_index + 1)
        .take(14)
        .filter(|(_, line)| !line.trim().is_empty())
        .collect::<Vec<_>>();
    for kind in ["in massa", "percentuale"] {
        for (position, (_, description_line)) in following.iter().enumerate() {
            let Some(description) = value_description_pattern().captures(description_line) else {
                continue;
            };
            if !description
                .name("kind")
                .is_some_and(|value| value.as_str().eq_ignore_ascii_case(kind))
                || position + 2 >= following.len()
            {
                continue;
            }
            let (_, value_line) = following[position + 1];
            let (unit_index, unit_line) = following[position + 2];
            let Some(value) = stacked_value_pattern().captures(value_line) else {
                continue;
            };
            let Some(unit) = stacked_unit_pattern().captures(unit_line) else {
                continue;
            };
            let Some(raw_value) = value.name("number").map(|item| item.as_str()) else {
                continue;
            };
            let raw_unit = unit_without_reference_range(unit.name("unit")?.as_str());
            if raw_unit.is_empty() {
                continue;
            }
            let candidate = candidate_from_line(
                document,
                href,
                digest,
                &format!(
                    "{label} {} {raw_value} {raw_unit}",
                    value.name("qualifier").map_or("", |item| item.as_str())
                ),
                label_index + 1,
                offsets[label_index],
                settings,
            )?;
            let unit_start = unit_line.rfind(&raw_unit)?;
            let end = offsets[unit_index]
                + unit_line[..unit_start].chars().count()
                + raw_unit.chars().count();
            return Some(finish_table_candidate(
                candidate,
                offsets[label_index],
                end,
                lines[label_index..=unit_index].join("\n"),
                raw_value,
                &raw_unit,
            ));
        }
    }
    None
        }
    )
}

fn finish_table_candidate(
    mut candidate: ParameterCandidate,
    start: usize,
    end: usize,
    original_line: String,
    raw_value: &str,
    raw_unit: &str,
) -> ParameterCandidate {
    trace_parameter_call!(
        "parameter_slices::finish_table_candidate",
        ["start" => &start, "end" => &end],
        fields [
            "candidate" => trace_debug(&candidate),
            "original_line" => trace_sensitive_text(&original_line),
            "raw_value" => trace_sensitive_text(raw_value),
            "raw_unit" => trace_sensitive_text(raw_unit),
        ],
        || {
    candidate.stable_id = sha256(
        [
            candidate.document_id.as_str(),
            candidate.source_text_digest.as_str(),
            &start.to_string(),
            &end.to_string(),
            candidate.normalized_label.as_str(),
            raw_value,
            raw_unit,
        ]
        .join("\u{1f}")
        .as_bytes(),
    );
    candidate.character_start = start;
    candidate.character_end = end;
    candidate.original_line = original_line;
    candidate.prefix_context.clear();
    candidate.suffix_context.clear();
    candidate
        }
    )
}

fn following_table_cells<'a>(
    lines: &[&'a str],
    label_index: usize,
    count: usize,
) -> Vec<(usize, &'a str)> {
    trace_parameter_call!(
        "parameter_slices::following_table_cells",
        ["label_index" => &label_index, "count" => &count],
        fields ["lines" => trace_lines(lines)],
        || {
    let mut cells = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(label_index + 1).take(12) {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || table_method_pattern().is_match(trimmed)
            || trimmed.eq_ignore_ascii_case("valore")
        {
            continue;
        }
        cells.push((index, *line));
        if cells.len() == count {
            break;
        }
    }
    cells
        }
    )
}

fn unit_without_reference_range(unit: &str) -> String {
    trace_parameter_call!("parameter_slices::unit_without_reference_range", [], fields ["unit" => trace_sensitive_text(unit)], || {
    reference_range_suffix_pattern()
        .replace(unit, "")
        .trim()
        .to_owned()
    })
}

fn parse_number(raw: &str) -> (Option<f64>, Option<String>, Option<String>) {
    trace_parameter_call!("parameter_slices::parse_number", [], fields ["raw" => trace_sensitive_text(raw)], || {
    if raw.chars().all(|ch| ch.is_ascii_digit()) {
        return (raw.parse().ok(), Some("integer".into()), None);
    }
    if raw.contains('.') && raw.contains(',') {
        let decimal = if raw.rfind('.') > raw.rfind(',') {
            '.'
        } else {
            ','
        };
        let grouping = if decimal == '.' { ',' } else { '.' };
        let Some((integer, fraction)) = raw.rsplit_once(decimal) else {
            return rejected_number();
        };
        let groups = integer.split(grouping).collect::<Vec<_>>();
        if !fraction.chars().all(|ch| ch.is_ascii_digit())
            || groups[0].is_empty()
            || groups[0].len() > 3
            || !groups
                .iter()
                .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
            || groups.iter().skip(1).any(|part| part.len() != 3)
        {
            return rejected_number();
        }
        let format = if decimal == '.' {
            "grouped-comma-decimal-point"
        } else {
            "grouped-point-decimal-comma"
        };
        return (
            format!("{}.{}", groups.concat(), fraction).parse().ok(),
            Some(format.into()),
            None,
        );
    }
    let decimal = if raw.contains('.') { '.' } else { ',' };
    let Some((integer, fraction)) = raw.split_once(decimal) else {
        return rejected_number();
    };
    if integer.is_empty()
        || fraction.is_empty()
        || !integer.chars().all(|ch| ch.is_ascii_digit())
        || !fraction.chars().all(|ch| ch.is_ascii_digit())
    {
        return rejected_number();
    }
    if fraction.len() == 3 {
        return (None, None, Some("REJECTED_AMBIGUOUS_NUMBER_FORMAT".into()));
    }
    let format = if decimal == '.' {
        "decimal-point"
    } else {
        "decimal-comma"
    };
    (
        format!("{integer}.{fraction}").parse().ok(),
        Some(format.into()),
        None,
    )
    })
}

fn rejected_number() -> (Option<f64>, Option<String>, Option<String>) {
    trace_parameter_call!("parameter_slices::rejected_number", [], || {
        (None, None, Some("REJECTED_INVALID_NUMBER_FORMAT".into()))
    })
}

fn label_is_eligible(label: &str, settings: &DiscoverySettings) -> bool {
    trace_parameter_call!("parameter_slices::label_is_eligible", [], fields ["label" => trace_sensitive_text(label), "settings" => trace_debug(settings)], || {
    let count = label.chars().count();
    count >= settings.min_label_length
        && count <= settings.max_label_length
        && label.split_whitespace().count() <= settings.max_label_words
        && label.chars().any(char::is_alphabetic)
        && !settings
            .excluded_labels
            .iter()
            .any(|item| normalize_label(item) == label)
    })
}

fn page_number(spans: &[TextPageSpan], offset: usize) -> Option<usize> {
    trace_parameter_call!("parameter_slices::page_number", ["offset" => &offset], fields ["spans" => trace_debug(spans)], || {
    spans
        .iter()
        .find(|span| span.character_start <= offset && offset < span.character_end)
        .map(|span| span.page_number)
    })
}

fn line_context(lines: &[&str], index: usize) -> String {
    trace_parameter_call!("parameter_slices::line_context", ["index" => &index], fields ["lines" => trace_lines(lines)], || {
    let start = index.saturating_sub(1);
    let end = (index + 1).min(lines.len().saturating_sub(1));
    lines[start..=end]
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join(" ⟪ ")
    })
}

fn section_specimen(lines: &[&str], index: usize) -> Option<String> {
    trace_parameter_call!("parameter_slices::section_specimen", ["index" => &index], fields ["lines" => trace_lines(lines)], || {
    for line in lines[..=index].iter().rev() {
        let label = normalize_label(line);
        if label.starts_with("u-esame") || (label.starts_with("esame") && label.contains("urine")) {
            return Some("urine".into());
        }
        if label.starts_with("s-esame") || label.contains("esami ematici") {
            return Some("serum".into());
        }
    }
    None
    })
}

fn proposed_groups(
    candidates: &[ParameterCandidate],
    settings: &DiscoverySettings,
) -> Vec<CandidateGroup> {
    trace_parameter_call!("parameter_slices::proposed_groups", ["settings" => settings], fields ["candidates" => trace_debug(candidates)], || {
    let mut grouped = BTreeMap::<String, Vec<ParameterCandidate>>::new();
    for item in candidates {
        grouped
            .entry(item.normalized_label.clone())
            .or_default()
            .push(item.clone());
    }
    grouped
        .into_iter()
        .filter_map(|(label, candidates)| {
            let group = CandidateGroup {
                normalized_label: label,
                candidates,
            };
            (group.candidates.len() >= settings.min_occurrences
                && group.distinct_documents() >= settings.min_distinct_documents
                && group.distinct_dates() >= settings.min_distinct_dates)
                .then_some(group)
        })
        .collect()
    })
}

fn validate_settings(settings: &DiscoverySettings) -> Result<(), String> {
    trace_parameter_call!("parameter_slices::validate_settings", [], fields ["settings" => trace_debug(settings)], || {
    if settings.min_label_length == 0 {
        return Err("min_label_length deve essere positivo".into());
    }
    if settings.max_label_length < settings.min_label_length {
        return Err("max_label_length non puo' essere minore del minimo".into());
    }
    if settings.max_label_words == 0 {
        return Err("max_label_words deve essere positivo".into());
    }
    if [
        settings.min_occurrences,
        settings.min_distinct_documents,
        settings.min_distinct_dates,
    ]
    .contains(&0)
    {
        return Err("le soglie di proposta devono essere positive".into());
    }
    Ok(())
    })
}

fn sha256(bytes: &[u8]) -> String {
    trace_parameter_call!("parameter_slices::sha256", [], fields ["input_bytes" => trace_bytes(bytes)], || {
        format!("{:x}", Sha256::digest(bytes))
    })
}

fn candidate_pattern() -> &'static Regex {
    trace_parameter_call!("parameter_slices::candidate_pattern", [], || {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| Regex::new(r"^(?P<label>.+?)(?P<separator>\s*(?::|=)\s*|\s+)(?P<qualifier><=|>=|<|>)?\s*(?P<number>\d{1,3}(?:[.,]\d{3})+[.,]\d+|\d+[.,]\d+|\d+)(?:\s*(?P<unit>[A-Za-z%µμ][A-Za-z0-9%µμ/._^*\-]*(?:\s*/\s*[A-Za-z%µμ][A-Za-z0-9%µμ._^*\-]*)?))?").expect("grammar costante valida"))
    })
}

fn stacked_value_pattern() -> &'static Regex {
    trace_parameter_call!("parameter_slices::stacked_value_pattern", [], || {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| {
        Regex::new(r"^\s*(?:\*\s*)?(?P<qualifier><=|>=|<|>)?\s*(?P<number>\d{1,3}(?:[.,]\d{3})+[.,]\d+|\d+[.,]\d+|\d+)\s*$")
            .expect("grammar costante valida")
    })
    })
}

fn stacked_unit_pattern() -> &'static Regex {
    trace_parameter_call!("parameter_slices::stacked_unit_pattern", [], || {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| {
            Regex::new(r#"^\s*(?P<unit>[A-Za-z%µμ][A-Za-z0-9%µμ/._^+*\-" ]*)\s*$"#)
                .expect("grammar costante valida")
        })
    })
}

fn reference_cell_pattern() -> &'static Regex {
    trace_parameter_call!("parameter_slices::reference_cell_pattern", [], || {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| {
        Regex::new(r"^\s*(?:\(\s*)?(?:<=|>=|<|>)?\s*\d+(?:[.,]\d+)?(?:\s*(?:-|–|a)\s*(?:<=|>=|<|>)?\s*\d+(?:[.,]\d+)?)?\s*\)?\s*$")
            .expect("grammar costante valida")
    })
    })
}

fn table_method_pattern() -> &'static Regex {
    trace_parameter_call!("parameter_slices::table_method_pattern", [], || {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| {
            Regex::new(r"(?i)^(?:met\.|metodo\s*:).*$").expect("grammar costante valida")
        })
    })
}

fn inline_result_pattern() -> &'static Regex {
    trace_parameter_call!("parameter_slices::inline_result_pattern", [], || {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| {
            Regex::new(r"(?i)^\s*valore\s+(?P<content>.+)$").expect("grammar costante valida")
        })
    })
}

fn value_description_pattern() -> &'static Regex {
    trace_parameter_call!("parameter_slices::value_description_pattern", [], || {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| {
            Regex::new(r"(?i)^\s*valore\s+(?P<kind>in\s+massa|percentuale)\s*$")
                .expect("grammatica descrizione valore costante valida")
        })
    })
}

fn glycated_hemoglobin_label_pattern() -> &'static Regex {
    trace_parameter_call!(
        "parameter_slices::glycated_hemoglobin_label_pattern",
        [],
        || {
            static PATTERN: OnceLock<Regex> = OnceLock::new();
            PATTERN.get_or_init(|| {
                Regex::new(r"(?i)\b(?:hb\s+glicosilat|hb\s+glicat|emoglobina\s+glicosilat)")
                    .expect("grammatica emoglobina glicata costante valida")
            })
        }
    )
}

fn reference_range_suffix_pattern() -> &'static Regex {
    trace_parameter_call!(
        "parameter_slices::reference_range_suffix_pattern",
        [],
        || {
            static PATTERN: OnceLock<Regex> = OnceLock::new();
            PATTERN.get_or_init(|| {
                Regex::new(r"\s+(?:[<>=~]\s*)?\d+(?:[.,]\d+)?\s*(?:-|–|a)\s*\d+(?:[.,]\d+)?\s*$")
                    .expect("grammar costante valida")
            })
        }
    )
}

fn trailing_separator() -> &'static Regex {
    trace_parameter_call!("parameter_slices::trailing_separator", [], || {
        static PATTERN: OnceLock<Regex> = OnceLock::new();
        PATTERN.get_or_init(|| Regex::new(r"\s*[:=]\s*$").expect("grammar costante valida"))
    })
}

#[cfg(test)]
mod tests {
    use super::{DiscoverySettings, discover_candidates, discover_configured_candidates};
    use crate::logging::{Component, Level, Session};
    use crate::metadata::DocumentRecord;
    use crate::pdf::{ExtractedText, TextPageSpan};
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn trace_directory() -> PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "sanikey-parameter-slices-trace-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn document(id: &str, date: Option<&str>) -> DocumentRecord {
        DocumentRecord {
            document_id: id.into(),
            patient_id: "synthetic-person".into(),
            path: PathBuf::from(format!("/{id}.txt")),
            title: format!("Report {id}"),
            category: "Laboratorio".into(),
            kind: "text".into(),
            sha256: id.repeat(8),
            date: date.map(str::to_owned),
            series: None,
            tags: Vec::new(),
            origin: "source".into(),
            container_id: None,
            internal_path: None,
        }
    }

    fn extraction(id: &str, text: &str) -> ExtractedText {
        ExtractedText {
            document_id: id.into(),
            text: text.into(),
            warnings: Vec::new(),
            page_spans: Vec::new(),
        }
    }

    fn permissive() -> DiscoverySettings {
        DiscoverySettings {
            min_occurrences: 1,
            min_distinct_documents: 1,
            min_distinct_dates: 1,
            ..DiscoverySettings::default()
        }
    }

    #[test]
    fn configured_discovery_skips_unconfigured_numeric_lines() {
        let result = discover_configured_candidates(
            &[document("doc-a", Some("2026-01-02"))],
            &[extraction(
                "doc-a",
                "Telefono segreteria 02 12345627\nHb: 13.7 g/dL\n",
            )],
            &["Hb".into(), "emoglobina".into()],
            None,
            &DiscoverySettings::default(),
        )
        .unwrap();
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|item| item.normalized_label.as_str())
                .collect::<Vec<_>>(),
            ["hb"]
        );
        assert!(result.proposed_groups.is_empty());
    }

    #[test]
    fn preserves_provenance_and_recognizes_numeric_formats() {
        let doc = document("doc-a", Some("2026-01-02"));
        let text = "Emoglobina 13,7 g/dL\nTAPSE = 22 mm\nFE: 1.234,56 %\nAzotemia 39 ma /AL\n";
        let hrefs = HashMap::from([("doc-a".into(), "../documents/report.txt".into())]);
        let result = discover_candidates(
            &[doc],
            &[extraction("doc-a", text)],
            Some(&hrefs),
            &permissive(),
            None,
        )
        .unwrap();
        assert_eq!(result.analyzed_lines, 4);
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|item| item.normalized_label.as_str())
                .collect::<Vec<_>>(),
            ["azotemia", "emoglobina", "fe", "tapse"]
        );
        let hb = result
            .candidates
            .iter()
            .find(|item| item.normalized_label == "emoglobina")
            .unwrap();
        assert_eq!(hb.parsed_value, Some(13.7));
        assert_eq!(hb.raw_unit.as_deref(), Some("g/dL"));
        assert_eq!(hb.document_href.as_deref(), Some("../documents/report.txt"));
        assert_eq!(hb.document_name, "doc-a.txt");
        assert_eq!(hb.line_number, 1);
        assert_eq!(hb.character_start, 0);
        assert_eq!(hb.original_line, "Emoglobina 13,7 g/dL");
        let fe = result
            .candidates
            .iter()
            .find(|item| item.normalized_label == "fe")
            .unwrap();
        assert_eq!(fe.parsed_value, Some(1234.56));
        assert_eq!(
            fe.number_format.as_deref(),
            Some("grouped-point-decimal-comma")
        );
        let nitrogen = result
            .candidates
            .iter()
            .find(|item| item.normalized_label == "azotemia")
            .unwrap();
        assert_eq!(nitrogen.raw_unit.as_deref(), Some("ma /AL"));
    }

    #[test]
    fn recognizes_vertical_stacked_cells_and_exponent_units() {
        let result = discover_candidates(
            &[document("doc-a", None)],
            &[extraction(
                "doc-a",
                "PLT (Piastrine)\n    159\n\nx10^3/mmc 140-440\n",
            )],
            None,
            &permissive(),
            None,
        )
        .expect("valid discovery");
        let candidate = &result.candidates[0];
        assert_eq!(candidate.normalized_label, "plt (piastrine)");
        assert_eq!(candidate.raw_value, "159");
        assert_eq!(candidate.raw_unit.as_deref(), Some("x10^3/mmc"));
        assert_eq!(
            candidate.original_line,
            "PLT (Piastrine)\n    159\n\nx10^3/mmc 140-440"
        );
    }

    #[test]
    fn recognizes_asterisked_stacked_values() {
        let result = discover_candidates(
            &[document("doc-a", None)],
            &[extraction("doc-a", "GLICEMIA\n*   120\nmg/dl\n70 - 100\n")],
            None,
            &permissive(),
            None,
        )
        .expect("valid discovery");
        assert_eq!(result.candidates[0].normalized_label, "glicemia");
        assert_eq!(result.candidates[0].raw_value, "120");
        assert_eq!(result.candidates[0].raw_unit.as_deref(), Some("mg/dl"));
    }

    #[test]
    fn recognizes_unit_reference_value_table_cells() {
        let result = discover_candidates(
            &[document("doc-a", None)],
            &[extraction(
                "doc-a",
                "GLICEMIA BASALE\nmg/dl\n(60,0-100,0)\n134\nEMOGLOBINA GLICOSILATA\n%\n<6\n7,0\nCREATININA\nmg/dl\n(0,72-1,25)\n1,06\n",
            )],
            None,
            &permissive(),
            None,
        )
        .expect("valid discovery");
        let observed = result
            .candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.normalized_label.as_str(),
                    candidate.raw_value.as_str(),
                    candidate.raw_unit.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            observed,
            [
                ("creatinina", "1,06", Some("mg/dl")),
                ("emoglobina glicosilata", "7,0", Some("%")),
                ("glicemia basale", "134", Some("mg/dl")),
            ]
        );
    }

    #[test]
    fn reads_annotated_value_after_method_lines() {
        let examples = [
            "CREATININA\nmet. Colorimetria\n\nvalore\n  0,93\n\nmg/dL\nadulto: 0,7 - 1,2\n",
            "CREATININA\nMetodo: Enzimatico\nvalore 0,93 mg/dL adulto: 0,7 - 1,2\n",
            "CREATININA\nmet. Colorimetria\nmg/dL\n(0,7-1,2)\n0,93\n",
        ];
        for text in examples {
            let result = discover_configured_candidates(
                &[document("doc-a", None)],
                &[extraction("doc-a", text)],
                &["creatinina".into()],
                None,
                &permissive(),
            )
            .expect("valid discovery");
            assert_eq!(result.candidates.len(), 1, "{text}");
            let candidate = &result.candidates[0];
            assert_eq!(candidate.raw_value, "0,93", "{text}");
            assert_eq!(candidate.raw_unit.as_deref(), Some("mg/dL"), "{text}");
            assert!(
                candidate.original_line.starts_with("CREATININA\n"),
                "{text}"
            );
            assert!(candidate.original_line.contains("0,93"), "{text}");
        }
    }

    #[test]
    fn prefers_glycated_hemoglobin_mass_cells_over_percentage_cells() {
        let text = concat!(
            "Hb GLICATA (HbA1c)\n",
            "met. Elettrof.capillare\n",
            "valore in massa\n",
            "41,0\n",
            "mmol/mol\n",
            "standardizzazione IFCC: 20 - 42\n",
            "valore percentuale\n",
            "5,9\n",
            "%\n",
        );
        let result = discover_candidates(
            &[document("doc-a", None)],
            &[extraction("doc-a", text)],
            None,
            &permissive(),
            None,
        )
        .expect("valid discovery");

        let candidates = result
            .candidates
            .iter()
            .filter(|candidate| candidate.normalized_label == "hb glicata (hba1c)")
            .collect::<Vec<_>>();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].raw_value, "41,0");
        assert_eq!(candidates[0].parsed_value, Some(41.0));
        assert_eq!(candidates[0].raw_unit.as_deref(), Some("mmol/mol"));
        assert!(
            candidates[0]
                .original_line
                .starts_with("Hb GLICATA (HbA1c)\n")
        );
        assert!(candidates[0].original_line.contains("41,0\nmmol/mol"));
        assert_eq!(
            &text[candidates[0].character_start..candidates[0].character_end],
            "Hb GLICATA (HbA1c)\nmet. Elettrof.capillare\nvalore in massa\n41,0\nmmol/mol"
        );
    }

    #[test]
    fn annotated_result_does_not_cross_into_the_next_label() {
        let result = discover_configured_candidates(
            &[document("doc-a", None)],
            &[extraction(
                "doc-a",
                "CREATININA\nmet. Colorimetria\nEmoglobina\nvalore 13,7 g/dL\n",
            )],
            &["creatinina".into(), "emoglobina".into()],
            None,
            &permissive(),
        )
        .expect("valid discovery");
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|candidate| candidate.normalized_label.as_str())
                .collect::<Vec<_>>(),
            ["emoglobina"]
        );
    }

    #[test]
    fn section_headings_mark_urine_results() {
        let result = discover_candidates(
            &[document("doc-a", None)],
            &[extraction(
                "doc-a",
                "ESAME COMPLETO DELLE URINE\nGlucosio 500 mg/dl\n",
            )],
            None,
            &permissive(),
            None,
        )
        .unwrap();
        assert_eq!(result.candidates[0].normalized_label, "glucosio");
        assert_eq!(
            result.candidates[0].section_specimen.as_deref(),
            Some("urine")
        );
    }

    #[test]
    fn ambiguous_single_separator_numbers_are_rejected() {
        let result = discover_candidates(
            &[document("doc-a", None)],
            &[extraction("doc-a", "Creatinina 1,234 mg/dL\n")],
            None,
            &permissive(),
            None,
        )
        .unwrap();
        let item = &result.candidates[0];
        assert_eq!(item.parsed_value, None);
        assert_eq!(item.number_format, None);
        assert_eq!(
            item.reason_code.as_deref(),
            Some("REJECTED_AMBIGUOUS_NUMBER_FORMAT")
        );
    }

    #[test]
    fn proposal_groups_require_exact_normalized_labels_and_thresholds() {
        let docs = [
            document("doc-a", Some("2026-01-02")),
            document("doc-b", Some("2026-02-03")),
        ];
        let texts = [
            extraction("doc-a", "Hb 13.7 g/dL\n"),
            extraction("doc-b", "Hb: 14.1 g/dL\nEmoglobina 14\n"),
        ];
        let result =
            discover_candidates(&docs, &texts, None, &DiscoverySettings::default(), None).unwrap();
        assert_eq!(
            result
                .proposed_groups
                .iter()
                .map(|item| item.normalized_label.as_str())
                .collect::<Vec<_>>(),
            ["hb"]
        );
        assert_eq!(result.proposed_groups[0].candidates.len(), 2);
    }

    #[test]
    fn labels_without_letters_and_configured_exclusions_are_dropped() {
        let settings = DiscoverySettings {
            excluded_labels: vec!["Protocollo".into()],
            ..permissive()
        };
        let result = discover_candidates(
            &[document("doc-a", None)],
            &[extraction("doc-a", "123 45\nProtocollo 12\n")],
            None,
            &settings,
            None,
        )
        .unwrap();
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn records_pdf_page_and_unicode_offsets() {
        let mut extracted = extraction("doc-a", "È Hb 13,7 g/dL\ncontesto\n");
        extracted.page_spans = vec![TextPageSpan {
            page_number: 3,
            character_start: 0,
            character_end: 16,
        }];
        let result = discover_candidates(
            &[document("doc-a", None)],
            &[extracted],
            None,
            &permissive(),
            None,
        )
        .unwrap();
        let item = &result.candidates[0];
        assert_eq!(item.character_start, 0);
        assert_eq!(item.page_number, Some(3));
        assert_eq!(item.context, "È Hb 13,7 g/dL ⟪ contesto");
    }

    #[test]
    fn trace_captures_private_discovery_data_only_at_trace_level_and_redacts_secrets() {
        let documents = [document("doc-a", Some("2026-01-02"))];
        let extracted = [extraction(
            "doc-a",
            "Password = synthetic-parameter-secret\nHb: 13.7 g/dL\n",
        )];
        let labels = ["Hb".to_owned()];
        let hrefs = HashMap::from([("doc-a".to_owned(), "../documents/report.txt".to_owned())]);

        for level in [Level::Events, Level::Trace] {
            let directory = trace_directory();
            let (session, path) =
                Session::create_in_directory_with_level(&directory, Component::Core, level)
                    .expect("sessione log sintetica");
            session.scoped(|| {
                let result =
                    discover_candidates(&documents, &extracted, Some(&hrefs), &permissive(), None)
                        .expect("discovery sintetica");
                assert_eq!(result.candidates.len(), 1);
                let configured = discover_configured_candidates(
                    &documents,
                    &extracted,
                    &labels,
                    Some(&hrefs),
                    &permissive(),
                )
                .expect("discovery configurata sintetica");
                assert_eq!(configured.candidates.len(), 1);
                assert!(configured.proposed_groups.is_empty());
            });
            session.finish(Component::Core).expect("chiusura log");
            drop(session);

            let contents = fs::read_to_string(&path).expect("log sintetico leggibile");
            if level == Level::Trace {
                assert!(contents.contains("parameter_slices::discover_candidates"));
                assert!(contents.contains("parameter_slices::discover_configured_candidates"));
                assert!(contents.contains("Hb: 13.7 g/dL"));
                assert!(contents.contains("secret_omitted"));
                assert!(!contents.contains("synthetic-parameter-secret"));
            } else {
                assert!(!contents.contains("parameter_slices::"));
                assert!(!contents.contains("Hb: 13.7 g/dL"));
            }
            fs::remove_file(path).expect("rimozione log");
            fs::remove_dir(directory).expect("rimozione directory log");
        }
    }

    #[cfg(unix)]
    #[test]
    fn document_input_trace_preserves_native_non_unicode_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let mut document = document("doc-a", None);
        document.path = PathBuf::from(OsString::from_vec(b"/synthetic/report-\xff.txt".to_vec()));
        let encoded = super::trace_documents(&[document]).to_json();
        assert_eq!(
            encoded["value"][0]["document"]["value"]["native_path"]["value"]["encoding"],
            "unix_bytes"
        );
        assert!(
            encoded["value"][0]["document"]["value"]["native_path"]["value"]["value"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(0xff))
        );
    }
}
