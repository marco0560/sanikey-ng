//! Scoperta deterministica di candidati a parametri clinici nei testi estratti.

use crate::metadata::DocumentRecord;
use crate::pdf::{ExtractedText, TextPageSpan};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

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
        self.candidates
            .iter()
            .map(|item| item.document_id.as_str())
            .collect::<HashSet<_>>()
            .len()
    }

    /// Conta le date autorevoli distinte.
    #[must_use]
    pub fn distinct_dates(&self) -> usize {
        self.candidates
            .iter()
            .filter_map(|item| item.document_date.as_deref())
            .collect::<HashSet<_>>()
            .len()
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
            ) else {
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

/// Normalizza l'etichetta comprimendo gli spazi e ignorando maiuscole/minuscole.
#[must_use]
pub fn normalize_label(value: &str) -> String {
    trailing_separator()
        .replace(value, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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

fn parse_number(raw: &str) -> (Option<f64>, Option<String>, Option<String>) {
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
}

fn rejected_number() -> (Option<f64>, Option<String>, Option<String>) {
    (None, None, Some("REJECTED_INVALID_NUMBER_FORMAT".into()))
}

fn label_is_eligible(label: &str, settings: &DiscoverySettings) -> bool {
    let count = label.chars().count();
    count >= settings.min_label_length
        && count <= settings.max_label_length
        && label.split_whitespace().count() <= settings.max_label_words
        && label.chars().any(char::is_alphabetic)
        && !settings
            .excluded_labels
            .iter()
            .any(|item| normalize_label(item) == label)
}

fn page_number(spans: &[TextPageSpan], offset: usize) -> Option<usize> {
    spans
        .iter()
        .find(|span| span.character_start <= offset && offset < span.character_end)
        .map(|span| span.page_number)
}

fn line_context(lines: &[&str], index: usize) -> String {
    let start = index.saturating_sub(1);
    let end = (index + 1).min(lines.len().saturating_sub(1));
    lines[start..=end]
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join(" ⟪ ")
}

fn section_specimen(lines: &[&str], index: usize) -> Option<String> {
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
}

fn proposed_groups(
    candidates: &[ParameterCandidate],
    settings: &DiscoverySettings,
) -> Vec<CandidateGroup> {
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
}

fn validate_settings(settings: &DiscoverySettings) -> Result<(), String> {
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
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn candidate_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^(?P<label>.+?)(?P<separator>\s*(?::|=)\s*|\s+)(?P<qualifier><=|>=|<|>)?\s*(?P<number>\d{1,3}(?:[.,]\d{3})+[.,]\d+|\d+[.,]\d+|\d+)(?:\s*(?P<unit>[A-Za-z%µμ][A-Za-z0-9%µμ/._^*\-]*(?:\s*/\s*[A-Za-z%µμ][A-Za-z0-9%µμ._^*\-]*)?))?").expect("grammar costante valida"))
}

fn trailing_separator() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\s*[:=]\s*$").expect("grammar costante valida"))
}

#[cfg(test)]
mod tests {
    use super::{DiscoverySettings, discover_candidates, discover_configured_candidates};
    use crate::metadata::DocumentRecord;
    use crate::pdf::{ExtractedText, TextPageSpan};
    use std::collections::HashMap;
    use std::path::PathBuf;

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
}
