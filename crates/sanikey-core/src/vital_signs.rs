//! Estrazione stretta di peso e pressione dal testo dei documenti.

use crate::metadata::{CuratedMetadata, DocumentRecord, ObservationPoint, ObservationSeries};
use crate::parameter_reports::ParameterDiagnosticExtra;
use crate::pdf::{ExtractedText, TextPageSpan};
use regex::{Captures, Regex};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

/// Segni vitali estratti e righe riconosciute ma rifiutate.
#[derive(Clone, Debug, PartialEq)]
pub struct VitalSignResult {
    /// Serie presenti in almeno un punto valido, nell'ordine canonico.
    pub series: Vec<ObservationSeries>,
    /// Punti con provenienza del documento e del testo estratto.
    pub points: Vec<ObservationPoint>,
    /// Diagnostiche compatibili con i report dei parametri.
    pub rejections: Vec<ParameterDiagnosticExtra>,
}

/// Estrae peso e pressione solo nelle forme esplicite previste dalla baseline.
///
/// Sono esaminati solo documenti presenti nell'input con una data. Le
/// espressioni supportate sono `Peso 70 kg` o `Peso corporeo: 70,5 kg`, e
/// `Pressione: 120/80 mmHg` o `PA 120/80 mm Hg`.
#[must_use]
pub fn extract_vital_signs(
    documents: &[DocumentRecord],
    extracted_text: &[ExtractedText],
    document_hrefs: Option<&HashMap<String, String>>,
) -> VitalSignResult {
    let records = documents
        .iter()
        .map(|document| (document.document_id.as_str(), document))
        .collect::<BTreeMap<_, _>>();
    let mut extractions = extracted_text.iter().collect::<Vec<_>>();
    extractions.sort_by(|left, right| left.document_id.cmp(&right.document_id));
    let mut points = Vec::new();
    let mut rejections = Vec::new();

    for extracted in extractions {
        let Some(document) = records.get(extracted.document_id.as_str()).copied() else {
            continue;
        };
        let Some(date) = document.date.as_deref() else {
            continue;
        };
        let digest = sha256(extracted.text.as_bytes());
        let mut byte_offset = 0;
        for (line_index, raw_line) in extracted.text.split('\n').enumerate() {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let line_number = line_index + 1;
            let character_line_start = extracted.text[..byte_offset].chars().count();
            let source = PointSource {
                document,
                date,
                extracted,
                hrefs: document_hrefs,
                text_digest: &digest,
                line,
                line_number,
                character_line_start,
            };
            for captures in weight_regex().captures_iter(line) {
                let value = captures["value"].replace(',', ".");
                let value = value.parse::<f64>().expect("valore peso riconosciuto");
                if (20.0..=400.0).contains(&value) {
                    points.push(build_point(&source, &captures, "peso", value, None));
                } else {
                    rejections.push(rejection(document, line_number, line, "peso"));
                }
            }
            for captures in pressure_regex().captures_iter(line) {
                let systolic = captures["systolic"]
                    .parse::<f64>()
                    .expect("sistolica riconosciuta");
                let diastolic = captures["diastolic"]
                    .parse::<f64>()
                    .expect("diastolica riconosciuta");
                if (60.0..=260.0).contains(&systolic)
                    && (30.0..=160.0).contains(&diastolic)
                    && systolic > diastolic
                {
                    points.push(build_point(
                        &source,
                        &captures,
                        "pressione",
                        systolic,
                        Some(diastolic),
                    ));
                } else {
                    rejections.push(rejection(document, line_number, line, "pressione"));
                }
            }
            byte_offset +=
                raw_line.len() + usize::from(byte_offset + raw_line.len() < extracted.text.len());
        }
    }

    points.sort_by(|left, right| {
        (&left.series_id, &left.observation_date, &left.id).cmp(&(
            &right.series_id,
            &right.observation_date,
            &right.id,
        ))
    });
    let ids = points
        .iter()
        .map(|point| point.series_id.as_str())
        .collect::<HashSet<_>>();
    let series = canonical_series()
        .into_iter()
        .filter(|series| ids.contains(series.id.as_str()))
        .collect();
    VitalSignResult {
        series,
        points,
        rejections,
    }
}

/// Unisce le serie estratte preservando i dati curati nello stesso giorno.
#[must_use]
pub fn merge_vital_signs(metadata: &CuratedMetadata, result: &VitalSignResult) -> CuratedMetadata {
    let mut merged = metadata.clone();
    let existing_series = metadata
        .observation_series
        .iter()
        .map(|series| series.id.as_str())
        .collect::<HashSet<_>>();
    merged.observation_series.extend(
        result
            .series
            .iter()
            .filter(|series| !existing_series.contains(series.id.as_str()))
            .cloned(),
    );
    let structured_dates = metadata
        .observation_points
        .iter()
        .filter(|point| point.source_kind == "curated-observation")
        .map(|point| (point.series_id.as_str(), point.observation_date.as_str()))
        .collect::<HashSet<_>>();
    merged.observation_points.extend(
        result
            .points
            .iter()
            .filter(|point| {
                !structured_dates
                    .contains(&(point.series_id.as_str(), point.observation_date.as_str()))
            })
            .cloned(),
    );
    merged
}

struct PointSource<'a> {
    document: &'a DocumentRecord,
    date: &'a str,
    extracted: &'a ExtractedText,
    hrefs: Option<&'a HashMap<String, String>>,
    text_digest: &'a str,
    line: &'a str,
    line_number: usize,
    character_line_start: usize,
}

fn build_point(
    source: &PointSource<'_>,
    captures: &Captures<'_>,
    series_id: &str,
    value: f64,
    diastolic: Option<f64>,
) -> ObservationPoint {
    let document = source.document;
    let date = source.date;
    let extracted = source.extracted;
    let hrefs = source.hrefs;
    let text_digest = source.text_digest;
    let line = source.line;
    let line_number = source.line_number;
    let character_line_start = source.character_line_start;
    let matched = captures.get(0).expect("corrispondenza completa");
    let raw = matched.as_str();
    let id_digest =
        sha256(format!("{}\x1f{}\x1f{raw}", document.document_id, line_number).as_bytes());
    let character_start = character_line_start + line[..matched.start()].chars().count();
    let character_end = character_start + raw.chars().count();
    let page = page_number(&extracted.page_spans, character_start);
    ObservationPoint {
        id: format!("vital-{}", &id_digest[..24]),
        series_id: series_id.into(),
        observation_date: date.into(),
        source_type: "document-text".into(),
        source_reference: format!("{}: riga {line_number}", document.title),
        numeric_value: diastolic.is_none().then_some(value),
        text_value: None,
        systolic: diastolic.map(|_| value),
        diastolic,
        pulse: None,
        note: None,
        source_kind: "document-extraction".into(),
        document_id: Some(document.document_id.clone()),
        document_href: hrefs
            .and_then(|items| items.get(&document.document_id))
            .cloned(),
        document_title: Some(document.title.clone()),
        document_category: Some(document.category.clone()),
        source_text_digest: Some(text_digest.into()),
        original_line: Some(line.into()),
        line_number: Some(line_number as i64),
        page_number: page.map(|page| page as i64),
        character_start: Some(character_start as i64),
        character_end: Some(character_end as i64),
        matched_label: Some(
            if series_id == "peso" {
                "Peso"
            } else {
                "Pressione"
            }
            .into(),
        ),
        raw_value: Some(raw.into()),
        parsed_value: Some(value),
        raw_unit: Some(if series_id == "peso" { "kg" } else { "mmHg" }.into()),
        normalized_unit: Some(if series_id == "peso" { "kg" } else { "mmHg" }.into()),
        qualifier: None,
        rule_id: None,
        rule_version: None,
        rule_digest: None,
        reason_code: Some("ACCEPTED_STRICT_VITAL_SIGN".into()),
    }
}

fn rejection(
    document: &DocumentRecord,
    line_number: usize,
    line: &str,
    parameter: &str,
) -> ParameterDiagnosticExtra {
    ParameterDiagnosticExtra {
        candidate_id: String::new(),
        parameter: parameter.into(),
        document: document.title.clone(),
        line: line_number,
        value: line.into(),
        reason_code: "REJECTED_OUT_OF_RANGE".into(),
    }
}

fn canonical_series() -> [ObservationSeries; 2] {
    [
        ObservationSeries {
            id: "peso".into(),
            name: "Peso".into(),
            value_type: "numeric".into(),
            unit: Some("kg".into()),
            description: None,
            warn_duplicate_same_day: false,
            synonyms: Vec::new(),
            parameter_rule_id: None,
            parameter_rule_version: None,
            parameter_rule_digest: None,
            unit_variant: None,
        },
        ObservationSeries {
            id: "pressione".into(),
            name: "Pressione".into(),
            value_type: "blood_pressure".into(),
            unit: Some("mmHg".into()),
            description: None,
            warn_duplicate_same_day: false,
            synonyms: Vec::new(),
            parameter_rule_id: None,
            parameter_rule_version: None,
            parameter_rule_digest: None,
            unit_variant: None,
        },
    ]
}

fn page_number(spans: &[TextPageSpan], character: usize) -> Option<usize> {
    spans
        .iter()
        .find(|span| span.character_start <= character && character < span.character_end)
        .map(|span| span.page_number)
}

fn weight_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:peso|peso\s+corporeo)\s*[:=]?\s*(?P<value>\d{2,3}(?:[.,]\d+)?)\s*(?:kg|chilogrammi?)\b",
        )
        .expect("grammatica peso costante valida")
    })
}

fn pressure_regex() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:pressione(?:\s+arteriosa)?|p\.?a\.?)\s*[:=]?\s*(?P<systolic>\d{2,3})\s*/\s*(?P<diastolic>\d{2,3})\s*(?:mm\s*hg|mmhg)\b",
        )
        .expect("grammatica pressione costante valida")
    })
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests;
