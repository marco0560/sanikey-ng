//! Report locali e artefatti statici delle slice dei parametri.

use crate::operation::{ErrorCode, OperationError};
use crate::parameter_rules::ParameterBuildResult;
use crate::parameter_slices::{CandidateGroup, DiscoveryResult, ParameterCandidate};
use chrono::{Local, SecondsFormat};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Percorsi degli artefatti generati dai report dei parametri.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterReportPaths {
    /// JSON dei candidati scoperti.
    pub candidates: PathBuf,
    /// TOML disabilitato per la revisione curata.
    pub proposed_rules: PathBuf,
    /// JSON delle serie, dei punti e delle decisioni, se generato.
    pub slices_json: Option<PathBuf>,
    /// JavaScript statico delle serie, dei punti e delle decisioni, se generato.
    pub slices_javascript: Option<PathBuf>,
}

/// Diagnostica esterna, per esempio un valore vitale riconosciuto ma rifiutato.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterDiagnosticExtra {
    /// Identificatore del candidato, vuoto se non applicabile.
    pub candidate_id: String,
    /// Nome del parametro o segno vitale.
    pub parameter: String,
    /// Documento sorgente leggibile.
    pub document: String,
    /// Riga sorgente.
    pub line: usize,
    /// Valore originale o riga da mostrare.
    pub value: String,
    /// Codice stabile del rifiuto.
    pub reason_code: String,
}

/// Scrive il report candidati, il TOML disabilitato per la revisione e, se
/// disponibile, gli export JSON e JavaScript delle serie derivate.
///
/// # Errors
///
/// Restituisce un errore se non è possibile creare le directory o scrivere uno
/// degli artefatti.
pub fn write_parameter_reports(
    build_root: impl AsRef<Path>,
    discovery: &DiscoveryResult,
    result: Option<&ParameterBuildResult>,
) -> Result<ParameterReportPaths, OperationError> {
    let build_root = build_root.as_ref();
    let reports = build_root.join("reports");
    create_directory(&reports)?;
    create_directory(&build_root.join("exports"))?;
    create_directory(&build_root.join("web/data"))?;

    let candidate_path = reports.join("parameter-candidates.json");
    let proposal_path = reports.join("parameter-rules.proposed.toml");
    let candidate_payload = json!({
        "schema_version": "1.0",
        "analyzed_lines": discovery.analyzed_lines,
        "candidates": discovery.candidates.iter().map(candidate_value).collect::<Vec<_>>(),
        "groups": discovery.proposed_groups.iter().map(group_value).collect::<Vec<_>>(),
    });
    write_json(&candidate_path, &candidate_payload)?;
    write_text(
        &proposal_path,
        &render_proposed_rules(&discovery.proposed_groups),
    )?;

    let Some(result) = result else {
        return Ok(ParameterReportPaths {
            candidates: candidate_path,
            proposed_rules: proposal_path,
            slices_json: None,
            slices_javascript: None,
        });
    };
    let (export, script) = write_parameter_extraction_reports(build_root, result)?;
    Ok(ParameterReportPaths {
        candidates: candidate_path,
        proposed_rules: proposal_path,
        slices_json: Some(export),
        slices_javascript: Some(script),
    })
}

/// Scrive gli export macchina JSON e JavaScript delle serie, dei punti e delle
/// decisioni già prodotte dalle regole curate.
///
/// # Errors
///
/// Restituisce un errore se non è possibile creare le directory o scrivere gli
/// artefatti.
pub fn write_parameter_extraction_reports(
    build_root: impl AsRef<Path>,
    result: &ParameterBuildResult,
) -> Result<(PathBuf, PathBuf), OperationError> {
    let build_root = build_root.as_ref();
    let reports = build_root.join("reports");
    let exports = build_root.join("exports");
    let data = build_root.join("web/data");
    create_directory(&reports)?;
    create_directory(&exports)?;
    create_directory(&data)?;

    let payload = json!({
        "schema_version": "1.0",
        "series": result.series.iter().map(series_value).collect::<Vec<_>>(),
        "points": result.points.iter().map(point_value).collect::<Vec<_>>(),
        "decisions": result.decisions.iter().map(|decision| json!({
            "candidate_id": decision.candidate_id,
            "rule_id": decision.rule_id,
            "accepted": decision.accepted,
            "reason_code": decision.reason_code,
        })).collect::<Vec<_>>(),
    });
    let export = exports.join("parameter-slices.json");
    write_json(&export, &payload)?;
    let script = data.join("parameter-slices.js");
    let compact = serde_json::to_vec(&payload).map_err(json_error)?;
    let compact = String::from_utf8(compact).map_err(|error| {
        OperationError::new(
            ErrorCode::Configuration,
            format!("serializzazione JavaScript parametri non valida: {error}"),
        )
    })?;
    write_text(
        &script,
        &format!("window.SANIKEY_PARAMETER_SLICES = {compact};\n"),
    )?;
    Ok((export, script))
}

/// Scrive report JSON e testuali separati per esiti accettati e rifiutati.
///
/// Le righe aggiuntive possono rappresentare rifiuti di estrattori correlati,
/// come i segni vitali. La funzione restituisce i due percorsi del report
/// rifiutato, come fa il generatore Python di riferimento.
///
/// # Errors
///
/// Restituisce un errore se la directory non è creabile o i report non sono
/// scrivibili.
pub fn write_parameter_rejections(
    build_root: impl AsRef<Path>,
    discovery: &DiscoveryResult,
    result: &ParameterBuildResult,
    extra: &[ParameterDiagnosticExtra],
) -> Result<(PathBuf, PathBuf), OperationError> {
    let reports = build_root.as_ref().join("reports");
    create_directory(&reports)?;
    let candidates = discovery
        .candidates
        .iter()
        .map(|candidate| (candidate.stable_id.as_str(), candidate))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for decision in &result.decisions {
        let Some(candidate) = candidates.get(decision.candidate_id.as_str()) else {
            continue;
        };
        let row = json!({
            "candidate_id": decision.candidate_id,
            "parameter": decision.rule_id,
            "document": candidate.document_name,
            "line": candidate.line_number,
            "page": candidate.page_number,
            "page_label": candidate.page_label,
            "context": candidate.context,
            "value": candidate.raw_value,
            "reason_code": decision.reason_code,
        });
        if decision.reason_code.starts_with("ACCEPTED_") {
            accepted.push(row);
        } else {
            rejected.push(row);
        }
    }
    for row in extra {
        let value = json!({
            "candidate_id": row.candidate_id,
            "parameter": row.parameter,
            "document": row.document,
            "line": row.line,
            "page": null,
            "page_label": "non disponibile",
            "context": "",
            "value": row.value,
            "reason_code": row.reason_code,
        });
        if row.reason_code.starts_with("ACCEPTED_") {
            accepted.push(value);
        } else {
            rejected.push(value);
        }
    }
    write_decision_report(&reports, "accepted", &accepted)?;
    let rejected_json = reports.join("parameter-rejected.json");
    let rejected_text = reports.join("parameter-rejected.txt");
    write_decision_report(&reports, "rejected", &rejected)?;
    Ok((rejected_json, rejected_text))
}

fn write_decision_report(
    reports: &Path,
    status: &str,
    rows: &[Value],
) -> Result<(), OperationError> {
    let json_path = reports.join(format!("parameter-{status}.json"));
    write_json(&json_path, &json!({"schema_version": "1.0", status: rows}))?;
    let headings = [
        "Parametro",
        "Documento",
        "Pagina",
        "Riga",
        "Valore",
        "Contesto",
        "Ragione",
    ];
    let columns = [
        "parameter",
        "document",
        "page_label",
        "line",
        "value",
        "context",
        "reason_code",
    ];
    let cells = rows
        .iter()
        .map(|row| columns.map(|key| json_cell(&row[key])))
        .collect::<Vec<_>>();
    let widths = std::array::from_fn::<_, 7, _>(|index| {
        cells
            .iter()
            .map(|row| row[index].chars().count())
            .chain(std::iter::once(headings[index].chars().count()))
            .max()
            .unwrap_or(0)
    });
    let mut lines = Vec::with_capacity(cells.len() + 1);
    lines.push(render_table_row(&headings.map(str::to_owned), &widths));
    lines.extend(cells.iter().map(|row| render_table_row(row, &widths)));
    write_text(
        &reports.join(format!("parameter-{status}.txt")),
        &format!("{}\n", lines.join("\n")),
    )
}

fn json_cell(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

fn render_table_row(row: &[String; 7], widths: &[usize; 7]) -> String {
    row.iter()
        .zip(widths)
        .map(|(cell, width)| format!("{cell:<width$}"))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn candidate_value(candidate: &ParameterCandidate) -> Value {
    json!({
        "stable_id": candidate.stable_id,
        "document_id": candidate.document_id,
        "document_date": candidate.document_date,
        "document_href": candidate.document_href,
        "document_title": candidate.document_title,
        "document_name": candidate.document_name,
        "document_category": candidate.document_category,
        "source_text_digest": candidate.source_text_digest,
        "line_number": candidate.line_number,
        "character_start": candidate.character_start,
        "character_end": candidate.character_end,
        "original_line": candidate.original_line,
        "normalized_label": candidate.normalized_label,
        "raw_value": candidate.raw_value,
        "parsed_value": candidate.parsed_value.filter(|value| value.is_finite()),
        "number_format": candidate.number_format,
        "qualifier": candidate.qualifier,
        "raw_unit": candidate.raw_unit,
        "prefix_context": candidate.prefix_context,
        "suffix_context": candidate.suffix_context,
        "reason_code": candidate.reason_code,
        "page_number": candidate.page_number,
        "context": candidate.context,
        "page_label": candidate.page_label,
        "section_specimen": candidate.section_specimen,
    })
}

fn group_value(group: &CandidateGroup) -> Value {
    json!({
        "normalized_label": group.normalized_label,
        "candidates": group.candidates.iter().map(candidate_value).collect::<Vec<_>>(),
    })
}

fn series_value(series: &crate::metadata::ObservationSeries) -> Value {
    json!({
        "id": series.id,
        "name": series.name,
        "value_type": series.value_type,
        "unit": series.unit,
        "description": series.description,
        "warn_duplicate_same_day": series.warn_duplicate_same_day,
        "synonyms": series.synonyms,
        "parameter_rule_id": series.parameter_rule_id,
        "parameter_rule_version": series.parameter_rule_version,
        "parameter_rule_digest": series.parameter_rule_digest,
        "unit_variant": series.unit_variant,
    })
}

fn point_value(point: &crate::metadata::ObservationPoint) -> Value {
    json!({
        "id": point.id,
        "series_id": point.series_id,
        "observation_date": point.observation_date,
        "source_type": point.source_type,
        "source_reference": point.source_reference,
        "numeric_value": point.numeric_value,
        "text_value": point.text_value,
        "systolic": point.systolic,
        "diastolic": point.diastolic,
        "pulse": point.pulse,
        "note": point.note,
        "source_kind": point.source_kind,
        "document_id": point.document_id,
        "document_href": point.document_href,
        "document_title": point.document_title,
        "document_category": point.document_category,
        "source_text_digest": point.source_text_digest,
        "original_line": point.original_line,
        "line_number": point.line_number,
        "page_number": point.page_number,
        "character_start": point.character_start,
        "character_end": point.character_end,
        "matched_label": point.matched_label,
        "raw_value": point.raw_value,
        "parsed_value": point.parsed_value,
        "raw_unit": point.raw_unit,
        "normalized_unit": point.normalized_unit,
        "qualifier": point.qualifier,
        "rule_id": point.rule_id,
        "rule_version": point.rule_version,
        "rule_digest": point.rule_digest,
        "reason_code": point.reason_code,
    })
}

fn render_proposed_rules(groups: &[CandidateGroup]) -> String {
    let mut output = String::from(
        "# Proposte determinate: revisionare prima di copiare in parameters.toml.\n\
         # Nessuna regola qui generata e' abilitata automaticamente.\n",
    );
    let generated_at = Local::now().to_rfc3339_opts(SecondsFormat::Secs, false);
    output.push_str(&format!("generated_at = {}\n", toml_string(&generated_at)));
    for group in groups {
        let formats = group
            .candidates
            .iter()
            .filter_map(|candidate| candidate.number_format.as_deref())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let units = group
            .candidates
            .iter()
            .filter_map(|candidate| candidate.raw_unit.as_deref())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let value_type = if group
            .candidates
            .iter()
            .any(|candidate| candidate.qualifier.is_some())
        {
            "qualified-scalar"
        } else {
            "scalar"
        };
        output.push_str(&format!(
            "\n# occorrenze={} documenti={} date={}\n[parameters.{}]\n",
            group.candidates.len(),
            group.distinct_documents(),
            group.distinct_dates(),
            toml_string(&group.normalized_label)
        ));
        output.push_str(&format!(
            "display_name = {}\nterm = {}\nversion = 1\nvalue_type = {value_type:?}\nnumber_formats = {}\nunit_policy = \"required\"\nenabled = false\n",
            toml_string(&title_case(&group.normalized_label)),
            toml_string(&group.normalized_label),
            toml_strings(&formats),
        ));
        if !units.is_empty() {
            output.push_str(&format!("units = {}\n", toml_strings(&units)));
        }
    }
    output
}

fn title_case(value: &str) -> String {
    let mut capitalize = true;
    value
        .chars()
        .flat_map(|character| {
            if character.is_alphanumeric() {
                let output = if capitalize {
                    character.to_uppercase().collect::<Vec<_>>()
                } else {
                    character.to_lowercase().collect::<Vec<_>>()
                };
                capitalize = false;
                output
            } else {
                capitalize = true;
                vec![character]
            }
        })
        .collect()
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("una stringa è sempre serializzabile in JSON/TOML")
}

fn toml_strings(values: &BTreeSet<String>) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| toml_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn create_directory(path: &Path) -> Result<(), OperationError> {
    fs::create_dir_all(path).map_err(|error| {
        OperationError::new(
            ErrorCode::Configuration,
            format!("creazione directory report parametri non riuscita: {error}"),
        )
    })
}

fn write_text(path: &Path, content: &str) -> Result<(), OperationError> {
    fs::write(path, content).map_err(|error| {
        OperationError::new(
            ErrorCode::Configuration,
            format!("scrittura report parametri non riuscita: {error}"),
        )
    })
}

fn write_json(path: &Path, payload: &Value) -> Result<(), OperationError> {
    let mut bytes = serde_json::to_vec_pretty(payload).map_err(json_error)?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|error| {
        OperationError::new(
            ErrorCode::Configuration,
            format!("scrittura JSON parametri non riuscita: {error}"),
        )
    })
}

fn json_error(error: serde_json::Error) -> OperationError {
    OperationError::new(
        ErrorCode::Configuration,
        format!("serializzazione JSON parametri non riuscita: {error}"),
    )
}

#[cfg(test)]
mod tests;
