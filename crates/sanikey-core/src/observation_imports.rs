//! Import tabellare delle osservazioni longitudinali.

use crate::logging::{
    Component, TraceField, TraceValue, trace_current, trace_sensitive_text, trace_toml_table,
    trace_toml_value,
};
use crate::metadata::{ObservationPoint, ObservationSeries};
use crate::operation::{ErrorCode, OperationError};
use calamine::{Data, Reader, open_workbook_auto};
use chrono::NaiveDate;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

const MANIFEST_FILE: &str = "observation_imports.toml";
const OUTPUT_DIR: &str = "observations/imported";

fn trace_observation_call<T>(
    function: &'static str,
    arguments: impl FnOnce() -> Vec<TraceField>,
    action: impl FnOnce() -> T,
    result: impl FnOnce(&T) -> TraceValue,
) -> T {
    let trace = trace_current(Component::Core, function, arguments);
    let value = action();
    if let Some(trace) = trace {
        let _ = trace.finish(result(&value));
    }
    value
}

fn trace_operation_result<T: std::fmt::Debug>(result: &Result<T, OperationError>) -> TraceValue {
    match result {
        Ok(value) => trace_sensitive_text(&format!("{value:#?}")),
        Err(error) => trace_error(error),
    }
}

fn trace_import_summary(result: &Result<ObservationImportResult, OperationError>) -> TraceValue {
    match result {
        Ok(value) => TraceValue::record(&[
            TraceField::new("state", TraceValue::data("ok")),
            TraceField::new("series_count", TraceValue::data(value.series.len() as u64)),
            TraceField::new("point_count", TraceValue::data(value.points.len() as u64)),
            TraceField::new(
                "warnings",
                trace_sensitive_text(&format!("{:#?}", value.warnings)),
            ),
            TraceField::new("content", trace_unmapped_cells_omission()),
        ]),
        Err(error) => trace_error(error),
    }
}

fn trace_points_result(result: &Result<Vec<ObservationPoint>, OperationError>) -> TraceValue {
    match result {
        Ok(points) => TraceValue::record(&[
            TraceField::new("state", TraceValue::data("ok")),
            TraceField::new("point_count", TraceValue::data(points.len() as u64)),
            // Source column names are no longer attached to transformed fields here.
            TraceField::new("content", trace_unmapped_cells_omission()),
        ]),
        Err(error) => trace_error(error),
    }
}

fn trace_error(error: &OperationError) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("code", TraceValue::data(error.code().as_str())),
        TraceField::new("message", trace_sensitive_text(error.message())),
    ])
}

fn trace_named_toml_value(field: &str, value: Option<&Value>) -> TraceValue {
    let mut table = Table::new();
    if let Some(value) = value {
        table.insert(field.to_owned(), value.clone());
    }
    trace_toml_table(&table)
}

fn trace_rows(rows: &[Row]) -> TraceValue {
    let values = rows
        .iter()
        .map(|row| {
            let mut table = Table::new();
            for (name, cell) in row {
                let value = match cell {
                    Cell::Empty => Value::String(String::new()),
                    Cell::Text(value) | Cell::Date(value) => Value::String(value.clone()),
                    Cell::Number(value) => Value::Float(*value),
                    Cell::Boolean(value) => Value::Boolean(*value),
                };
                table.insert(name.clone(), value);
            }
            Value::Table(table)
        })
        .collect();
    trace_toml_value(&Value::Array(values))
}

fn trace_unmapped_cells_omission() -> TraceValue {
    trace_omission("source_column_provenance_unavailable")
}

fn trace_omission(reason: &'static str) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("state", TraceValue::data("omitted")),
        TraceField::new("reason", TraceValue::data(reason)),
    ])
}

fn trace_cell(field: &str, cell: Option<&Cell>) -> TraceValue {
    let value = match cell {
        None | Some(Cell::Empty) => Value::String(String::new()),
        Some(Cell::Text(value) | Cell::Date(value)) => Value::String(value.clone()),
        Some(Cell::Number(value)) => Value::Float(*value),
        Some(Cell::Boolean(value)) => Value::Boolean(*value),
    };
    trace_named_toml_value(field, Some(&value))
}

/// Risultato normalizzato di un'importazione di osservazioni.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObservationImportResult {
    /// Serie dichiarate nel manifest.
    pub series: Vec<ObservationSeries>,
    /// Punti prodotti dalle fonti tabellari.
    pub points: Vec<ObservationPoint>,
    /// Avvisi non bloccanti, inclusi i duplicati giornalieri.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
struct SeriesSpec {
    series: ObservationSeries,
}

#[derive(Clone, Debug)]
struct ExtractSpec {
    series_id: String,
    columns: HashMap<String, String>,
    source_reference: String,
    note_columns: Vec<String>,
    note_join: String,
    static_note: Option<String>,
    date_policy: String,
    skip_invalid_dates: bool,
    compound_column: Option<String>,
    compound_pattern: Option<Regex>,
}

#[derive(Clone, Debug)]
struct SourceSpec {
    path: PathBuf,
    raw_path: String,
    sheet: Option<String>,
    header_rows: Vec<usize>,
    data_start_row: Option<usize>,
    fill_down: HashSet<String>,
    layout: String,
    matrix: Table,
    extracts: Vec<ExtractSpec>,
}

#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Empty,
    Text(String),
    Number(f64),
    Date(String),
    Boolean(bool),
}

type Row = HashMap<String, Cell>;

/// Importa manifest e fonti e salva gli artefatti in una directory separata dai metadati curati.
///
/// I file generati si trovano in `metadata/observations/imported/`; l'importazione non
/// modifica `observations/series.toml` né gli altri file curati.
///
/// # Errors
///
/// Restituisce un errore `configuration` se il manifest, una fonte o l'output non è valido.
pub fn import_observations(
    metadata_dir: impl AsRef<Path>,
    source_documents_dir: impl AsRef<Path>,
) -> Result<ObservationImportResult, OperationError> {
    let metadata_dir = metadata_dir.as_ref();
    trace_observation_call(
        "observation_imports::import_observations",
        || {
            vec![TraceField::new(
                "metadata_dir",
                TraceValue::path(metadata_dir),
            )]
        },
        || {
            let manifest_path = metadata_dir.join(MANIFEST_FILE);
            if !manifest_path.exists() {
                return Ok(ObservationImportResult::default());
            }
            let manifest_text = read_text(&manifest_path)?;
            let manifest = parse_manifest(&manifest_text, &manifest_path)?;
            let root = manifest
                .as_table()
                .ok_or_else(|| config_error("il manifest osservazioni deve essere una tabella"))?;
            let series = parse_series(root, &manifest_path)?;
            let source_documents_dir = source_documents_dir.as_ref();
            let sources = parse_sources(root, &manifest_path, source_documents_dir)?;
            let series_by_id: HashMap<_, _> = series
                .iter()
                .map(|item| (item.series.id.as_str(), &item.series))
                .collect();
            for source in &sources {
                for extract in &source.extracts {
                    if !series_by_id.contains_key(extract.series_id.as_str()) {
                        return Err(config_error(format!(
                            "{}: source {} referenzia series_id sconosciuto {}",
                            manifest_path.display(),
                            source.raw_path,
                            extract.series_id
                        )));
                    }
                }
            }
            let source_hashes = hash_sources(&sources)?;
            let mut result = ObservationImportResult {
                series: series.iter().map(|item| item.series.clone()).collect(),
                ..ObservationImportResult::default()
            };
            for (source_index, source) in sources.iter().enumerate() {
                let rows = read_source(source)?;
                result
                    .points
                    .extend(points_from_rows(source, &rows, source_index)?);
            }
            result.points.sort_by(|left, right| {
                (&left.series_id, &left.observation_date, &left.id).cmp(&(
                    &right.series_id,
                    &right.observation_date,
                    &right.id,
                ))
            });
            for item in &series {
                if item.series.warn_duplicate_same_day {
                    result
                        .warnings
                        .extend(duplicate_warnings(&item.series, &result.points));
                }
            }
            write_outputs(metadata_dir, &result, &manifest_text, &source_hashes)?;
            Ok(result)
        },
        trace_import_summary,
    )
}

/// Interrompe l'uso delle osservazioni importate quando manifest o fonti sono cambiati.
///
/// # Errors
///
/// Restituisce un errore `configuration` se gli artefatti mancano, non sono validi o sono obsoleti.
pub fn ensure_observation_imports_current(
    metadata_dir: impl AsRef<Path>,
    source_documents_dir: impl AsRef<Path>,
) -> Result<(), OperationError> {
    let metadata_dir = metadata_dir.as_ref();
    trace_observation_call(
        "observation_imports::ensure_observation_imports_current",
        || {
            vec![TraceField::new(
                "metadata_dir",
                TraceValue::path(metadata_dir),
            )]
        },
        || {
            let manifest_path = metadata_dir.join(MANIFEST_FILE);
            if !manifest_path.exists() {
                return Ok(());
            }
            let manifest_text = read_text(&manifest_path)?;
            let manifest = parse_manifest(&manifest_text, &manifest_path)?;
            let root = manifest
                .as_table()
                .ok_or_else(|| config_error("il manifest osservazioni deve essere una tabella"))?;
            let sources = parse_sources(root, &manifest_path, source_documents_dir.as_ref())?;
            let expected_sources = hash_sources(&sources)?;
            let output = metadata_dir.join(OUTPUT_DIR);
            let state_path = output.join("import_state.toml");
            let stale = || {
                config_error(format!(
                    "osservazioni importate non aggiornate: eseguire nuovamente l'importazione (`{}`)",
                    manifest_path.display()
                ))
            };
            if !output.is_dir()
                || !state_path.is_file()
                || !output.join("series.toml").is_file()
                || !output.join("points.toml").is_file()
            {
                return Err(stale());
            }
            let state_text = read_text(&state_path)?;
            let state: Value = toml::from_str(&state_text).map_err(|_| stale())?;
            let state = state.as_table().ok_or_else(stale)?;
            let manifest_hash = sha256(manifest_text.as_bytes());
            if state.get("manifest_hash").and_then(Value::as_str) != Some(manifest_hash.as_str()) {
                return Err(stale());
            }
            let actual_sources = state
                .get("source_hashes")
                .and_then(Value::as_table)
                .ok_or_else(stale)?;
            let actual: BTreeMap<_, _> = actual_sources
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|hash| (key.clone(), hash.to_owned()))
                })
                .collect();
            if actual != expected_sources {
                return Err(stale());
            }
            Ok(())
        },
        trace_operation_result,
    )
}

fn parse_series(root: &Table, path: &Path) -> Result<Vec<SeriesSpec>, OperationError> {
    trace_observation_call(
        "observation_imports::parse_series",
        || {
            vec![
                TraceField::new("manifest", trace_toml_table(root)),
                TraceField::new("path", TraceValue::path(path)),
            ]
        },
        || {
            let values = array(root, "series", path)?;
            let mut seen = HashSet::new();
            let mut series = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let item = table(value, path, "series", index)?;
                let id = required_string(item, "id", path, index)?;
                if !seen.insert(id.clone()) {
                    return Err(config_error(format!(
                        "{}: id serie osservazioni duplicato: {id}",
                        path.display()
                    )));
                }
                series.push(SeriesSpec {
                    series: ObservationSeries {
                        id,
                        name: required_string(item, "name", path, index)?,
                        value_type: required_string(item, "value_type", path, index)?,
                        unit: optional_string(item, "unit", path)?,
                        description: optional_string(item, "description", path)?,
                        warn_duplicate_same_day: optional_bool(
                            item,
                            "warn_duplicate_same_day",
                            true,
                            path,
                            index,
                        )?,
                        synonyms: Vec::new(),
                        parameter_rule_id: None,
                        parameter_rule_version: None,
                        parameter_rule_digest: None,
                        unit_variant: None,
                    },
                });
            }
            Ok(series)
        },
        trace_operation_result,
    )
}

fn parse_manifest(text: &str, path: &Path) -> Result<Value, OperationError> {
    trace_observation_call(
        "observation_imports::parse_manifest",
        || {
            vec![
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("raw_toml", trace_sensitive_text(text)),
            ]
        },
        || {
            toml::from_str(text).map_err(|error| {
                config_error(format!("TOML non valido in {}: {error}", path.display()))
            })
        },
        |result| match result {
            Ok(value) => trace_toml_value(value),
            Err(error) => trace_error(error),
        },
    )
}

fn parse_sources(
    root: &Table,
    manifest_path: &Path,
    documents: &Path,
) -> Result<Vec<SourceSpec>, OperationError> {
    trace_observation_call(
        "observation_imports::parse_sources",
        || {
            vec![
                TraceField::new("manifest", trace_toml_table(root)),
                TraceField::new("manifest_path", TraceValue::path(manifest_path)),
                TraceField::new("documents_dir", TraceValue::path(documents)),
            ]
        },
        || {
            let values = array(root, "source", manifest_path)?;
            let mut sources = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let item = table(value, manifest_path, "source", index)?;
                let raw_path = required_string(item, "path", manifest_path, index)?;
                let relative = Path::new(&raw_path);
                let source_path = if relative.is_absolute() {
                    relative.to_path_buf()
                } else {
                    documents.join(relative)
                };
                if !source_path.is_file() {
                    return Err(config_error(format!(
                        "{}: sorgente osservazioni non trovata: {}",
                        manifest_path.display(),
                        source_path.display()
                    )));
                }
                let extracts = parse_extracts(item, manifest_path, index)?;
                let header_rows = match item.get("header_rows") {
                    Some(Value::Array(rows)) if !rows.is_empty() => rows
                        .iter()
                        .map(|value| positive_usize(value, manifest_path, "header_rows", index))
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(_) => {
                        return Err(config_error(format!(
                            "{}: source {index} campo header_rows non valido",
                            manifest_path.display()
                        )));
                    }
                    None => vec![optional_usize(item, "header_row", 1, manifest_path, index)?],
                };
                let fill_down =
                    string_array(item.get("fill_down"), manifest_path, "fill_down", index)?
                        .into_iter()
                        .collect();
                let matrix = match item.get("matrix") {
                    None => Table::new(),
                    Some(Value::Table(value)) => value.clone(),
                    Some(_) => {
                        return Err(config_error(format!(
                            "{}: source {index} campo matrix deve essere una tabella",
                            manifest_path.display()
                        )));
                    }
                };
                sources.push(SourceSpec {
                    path: source_path,
                    raw_path,
                    sheet: optional_string(item, "sheet", manifest_path)?,
                    header_rows,
                    data_start_row: optional_usize_opt(
                        item,
                        "data_start_row",
                        manifest_path,
                        index,
                    )?,
                    fill_down,
                    layout: optional_string(item, "layout", manifest_path)?
                        .unwrap_or_else(|| "table".into()),
                    matrix,
                    extracts,
                });
            }
            Ok(sources)
        },
        trace_operation_result,
    )
}

fn parse_extracts(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<Vec<ExtractSpec>, OperationError> {
    trace_observation_call(
        "observation_imports::parse_extracts",
        || {
            vec![
                TraceField::new("source", trace_toml_table(item)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || match item.get("extract") {
            None => Ok(vec![parse_extract(item, item, path, index)?]),
            Some(Value::Array(values)) => values
                .iter()
                .enumerate()
                .map(|(offset, value)| {
                    let extract = table(value, path, "source.extract", offset)?;
                    parse_extract(extract, item, path, index)
                })
                .collect(),
            Some(_) => Err(config_error(format!(
                "{}: source {index} campo extract deve essere un array",
                path.display()
            ))),
        },
        trace_operation_result,
    )
}

fn parse_extract(
    item: &Table,
    parent: &Table,
    path: &Path,
    index: usize,
) -> Result<ExtractSpec, OperationError> {
    trace_observation_call(
        "observation_imports::parse_extract",
        || {
            vec![
                TraceField::new("extract", trace_toml_table(item)),
                TraceField::new("source", trace_toml_table(parent)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || {
            let columns = item
                .get("columns")
                .and_then(Value::as_table)
                .ok_or_else(|| {
                    config_error(format!(
                        "{}: source {index} deve contenere [source.columns]",
                        path.display()
                    ))
                })?;
            let columns = columns
                .iter()
                .map(|(key, value)| {
                    value
                        .as_str()
                        .map(|name| (key.clone(), name.to_owned()))
                        .ok_or_else(|| {
                            config_error(format!(
                                "{}: mappatura colonna {key} deve essere una stringa",
                                path.display()
                            ))
                        })
                })
                .collect::<Result<HashMap<_, _>, _>>()?;
            let source_reference = optional_string(item, "source_reference", path)?
                .or(optional_string(parent, "source_reference", path)?)
                .unwrap_or(required_string(parent, "path", path, index)?);
            let compound = match item.get("compound_value") {
                None => None,
                Some(Value::Table(table)) => Some(table),
                Some(_) => {
                    return Err(config_error(format!(
                        "{}: source {index} campo compound_value deve essere una tabella",
                        path.display()
                    )));
                }
            };
            let compound_column = compound
                .and_then(|table| table.get("column"))
                .map(|_| {
                    required_string(
                        compound.expect("tabella composta presente"),
                        "column",
                        path,
                        index,
                    )
                })
                .transpose()?;
            let compound_pattern = compound
                .and_then(|table| table.get("pattern"))
                .map(|_| {
                    required_string(
                        compound.expect("tabella composta presente"),
                        "pattern",
                        path,
                        index,
                    )
                })
                .transpose()?
                .map(|pattern| {
                    Regex::new(&pattern).map_err(|error| {
                        config_error(format!(
                            "{}: regex composto non valido: {error}",
                            path.display()
                        ))
                    })
                })
                .transpose()?;
            Ok(ExtractSpec {
                series_id: required_string(item, "series_id", path, index)?,
                columns,
                source_reference,
                note_columns: string_array(item.get("note_columns"), path, "note_columns", index)?,
                note_join: optional_string(item, "note_join", path)?.unwrap_or_else(|| "; ".into()),
                static_note: optional_string(item, "static_note", path)?,
                date_policy: optional_string(item, "date_policy", path)?
                    .unwrap_or_else(|| "exact".into()),
                skip_invalid_dates: optional_bool(item, "skip_invalid_dates", false, path, index)?,
                compound_column,
                compound_pattern,
            })
        },
        trace_operation_result,
    )
}

fn read_source(source: &SourceSpec) -> Result<Vec<Row>, OperationError> {
    trace_observation_call(
        "observation_imports::read_source",
        || {
            vec![
                TraceField::new("path", TraceValue::path(&source.path)),
                TraceField::new("raw_path", trace_sensitive_text(&source.raw_path)),
                TraceField::new("layout", TraceValue::data(source.layout.as_str())),
                TraceField::new(
                    "sheet",
                    trace_sensitive_text(&format!("{:?}", source.sheet)),
                ),
                TraceField::new("matrix", trace_toml_table(&source.matrix)),
                TraceField::new("header_rows", TraceValue::data(source.header_rows.clone())),
            ]
        },
        || {
            let suffix = source
                .path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let raw_rows = if suffix == "csv" {
                parse_csv(&read_text(&source.path)?)?
            } else if ["ods", "xls", "xlsb", "xlsm", "xlsx"].contains(&suffix.as_str()) {
                let mut workbook = open_workbook_auto(&source.path).map_err(|error| {
                    config_error(format!(
                        "lettura osservazioni non riuscita per {}: {error}",
                        source.path.display()
                    ))
                })?;
                let sheet_name = source
                    .sheet
                    .clone()
                    .or_else(|| workbook.sheet_names().first().cloned())
                    .ok_or_else(|| {
                        config_error(format!(
                            "{}: il foglio di calcolo non contiene worksheet",
                            source.path.display()
                        ))
                    })?;
                let range = workbook.worksheet_range(&sheet_name).map_err(|error| {
                    config_error(format!(
                        "lettura osservazioni non riuscita per {}: {error}",
                        source.path.display()
                    ))
                })?;
                range
                    .rows()
                    .map(|row| row.iter().map(cell_from_calamine).collect())
                    .collect()
            } else {
                return Err(config_error(format!(
                    "formato osservazioni non supportato: {}",
                    source.path.display()
                )));
            };
            if source.layout == "repeating_matrix" {
                return read_repeating_matrix(source, &raw_rows);
            }
            if source.layout != "table" {
                return Err(config_error(format!(
                    "layout osservazioni non supportato: {}",
                    source.layout
                )));
            }
            let headers = headers_from_rows(&raw_rows, &source.header_rows);
            let start = source
                .data_start_row
                .unwrap_or_else(|| source.header_rows.iter().copied().max().unwrap_or(1) + 1);
            Ok(raw_rows
                .into_iter()
                .skip(start.saturating_sub(1))
                .map(|values| {
                    headers
                        .iter()
                        .enumerate()
                        .filter(|(_, header)| !header.is_empty())
                        .map(|(index, header)| {
                            (
                                header.clone(),
                                values.get(index).cloned().unwrap_or(Cell::Empty),
                            )
                        })
                        .collect()
                })
                .collect())
        },
        |result| match result {
            Ok(rows) => trace_rows(rows),
            Err(error) => trace_error(error),
        },
    )
}

fn cell_from_calamine(value: &Data) -> Cell {
    trace_observation_call(
        "observation_imports::cell_from_calamine",
        || {
            vec![TraceField::new(
                "source_cell",
                trace_unmapped_cells_omission(),
            )]
        },
        || match value {
            Data::Empty => Cell::Empty,
            Data::String(value) => Cell::Text(value.trim().to_owned()),
            Data::Int(value) => Cell::Number(*value as f64),
            Data::Float(value) => Cell::Number(*value),
            Data::Bool(value) => Cell::Boolean(*value),
            Data::DateTime(value) => {
                let (year, month, day, _, _, _, _) = value.to_ymd_hms_milli();
                Cell::Date(format!("{year:04}-{month:02}-{day:02}"))
            }
            Data::DateTimeIso(value) => Cell::Text(value.clone()),
            Data::DurationIso(value) => Cell::Text(value.clone()),
            Data::Error(value) => Cell::Text(value.to_string()),
        },
        |cell| {
            TraceValue::record(&[
                TraceField::new(
                    "cell_type",
                    TraceValue::data(match cell {
                        Cell::Empty => "empty",
                        Cell::Text(_) => "text",
                        Cell::Number(_) => "number",
                        Cell::Date(_) => "date",
                        Cell::Boolean(_) => "boolean",
                    }),
                ),
                TraceField::new("cell_value", trace_unmapped_cells_omission()),
                TraceField::new("source_column", trace_unmapped_cells_omission()),
            ])
        },
    )
}

fn parse_csv(text: &str) -> Result<Vec<Vec<Cell>>, OperationError> {
    trace_observation_call(
        "observation_imports::parse_csv",
        || {
            vec![
                TraceField::new("raw_csv", trace_unmapped_cells_omission()),
                TraceField::new("byte_length", TraceValue::data(text.len() as u64)),
            ]
        },
        || {
            let mut rows = Vec::new();
            let mut row = Vec::new();
            let mut field = String::new();
            let mut chars = text.chars().peekable();
            let mut quoted = false;
            while let Some(ch) = chars.next() {
                match ch {
                    '"' if quoted && chars.peek() == Some(&'"') => {
                        field.push('"');
                        chars.next();
                    }
                    '"' => quoted = !quoted,
                    ',' if !quoted => {
                        row.push(csv_cell(std::mem::take(&mut field)));
                    }
                    '\n' if !quoted => {
                        row.push(csv_cell(std::mem::take(&mut field)));
                        rows.push(std::mem::take(&mut row));
                    }
                    '\r' if !quoted => {
                        if chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                        row.push(csv_cell(std::mem::take(&mut field)));
                        rows.push(std::mem::take(&mut row));
                    }
                    _ => field.push(ch),
                }
            }
            if quoted {
                return Err(config_error("CSV osservazioni con virgolette non chiuse"));
            }
            if !field.is_empty() || !row.is_empty() {
                row.push(csv_cell(field));
                rows.push(row);
            }
            Ok(rows)
        },
        |result| match result {
            Ok(rows) => TraceValue::record(&[
                TraceField::new("row_count", TraceValue::data(rows.len() as u64)),
                TraceField::new("raw_cells", trace_unmapped_cells_omission()),
            ]),
            Err(error) => trace_error(error),
        },
    )
}

fn csv_cell(value: String) -> Cell {
    let trace = trace_current(Component::Core, "observation_imports::csv_cell", || {
        vec![TraceField::new(
            "cell_value_without_header",
            trace_unmapped_cells_omission(),
        )]
    });
    let cell = if value.is_empty() {
        Cell::Empty
    } else {
        Cell::Text(value)
    };
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::record(&[
            TraceField::new(
                "cell_type",
                TraceValue::data(match &cell {
                    Cell::Empty => "empty",
                    Cell::Text(_) => "text",
                    Cell::Number(_) => "number",
                    Cell::Date(_) => "date",
                    Cell::Boolean(_) => "boolean",
                }),
            ),
            TraceField::new("cell_value", trace_unmapped_cells_omission()),
            TraceField::new("source_column", trace_unmapped_cells_omission()),
        ]));
    }
    cell
}

fn headers_from_rows(rows: &[Vec<Cell>], header_rows: &[usize]) -> Vec<String> {
    trace_observation_call(
        "observation_imports::headers_from_rows",
        || {
            vec![
                TraceField::new("header_rows", TraceValue::data(header_rows.to_vec())),
                TraceField::new("header_cells", {
                    let values = header_rows
                        .iter()
                        .filter_map(|number| rows.get(number.saturating_sub(1)))
                        .map(|row| {
                            serde_json::Value::Array(
                                row.iter()
                                    .map(|cell| trace_cell("header", Some(cell)).to_json())
                                    .collect(),
                            )
                        })
                        .collect();
                    TraceValue::Data(serde_json::Value::Array(values))
                }),
                TraceField::new("source_column_provenance", trace_unmapped_cells_omission()),
            ]
        },
        || {
            let width = header_rows
                .iter()
                .filter_map(|number| rows.get(number.saturating_sub(1)))
                .map(Vec::len)
                .max()
                .unwrap_or(0);
            let mut carried_rows = Vec::new();
            for number in header_rows {
                let raw = rows.get(number.saturating_sub(1));
                let mut previous = String::new();
                let carried = (0..width)
                    .map(|column| {
                        let value = raw
                            .and_then(|row| row.get(column))
                            .and_then(cell_text)
                            .unwrap_or_default();
                        if !value.is_empty() {
                            previous = value;
                        }
                        previous.clone()
                    })
                    .collect::<Vec<_>>();
                carried_rows.push(carried);
            }
            (0..width)
                .map(|column| {
                    let mut parts = Vec::new();
                    for row in &carried_rows {
                        if let Some(value) = row.get(column).filter(|value| !value.is_empty())
                            && !parts.contains(value)
                        {
                            parts.push(value.clone());
                        }
                    }
                    if parts.is_empty() {
                        format!("column_{}", column + 1)
                    } else {
                        parts.join(" ")
                    }
                })
                .collect()
        },
        |headers: &Vec<String>| trace_sensitive_text(&format!("{headers:#?}")),
    )
}

fn read_repeating_matrix(
    source: &SourceSpec,
    rows: &[Vec<Cell>],
) -> Result<Vec<Row>, OperationError> {
    trace_observation_call(
        "observation_imports::read_repeating_matrix",
        || {
            vec![
                TraceField::new("path", TraceValue::path(&source.path)),
                TraceField::new("matrix", trace_toml_table(&source.matrix)),
                TraceField::new("raw_cells_without_header", trace_unmapped_cells_omission()),
            ]
        },
        || {
            let Some(year) = table_int(&source.matrix, "year") else {
                return Err(config_error(format!(
                    "{}: matrix.year mancante o non valido",
                    source.raw_path
                )));
            };
            let mut month = table_int(&source.matrix, "start_month").ok_or_else(|| {
                config_error(format!(
                    "{}: matrix.start_month mancante o non valido",
                    source.raw_path
                ))
            })?;
            let value_rows = string_array(
                source.matrix.get("value_rows"),
                Path::new(&source.raw_path),
                "value_rows",
                0,
            )?;
            if value_rows.is_empty() {
                return Err(config_error(format!(
                    "{}: matrix.value_rows deve contenere almeno una riga",
                    source.raw_path
                )));
            }
            let block_height =
                table_int(&source.matrix, "block_height").unwrap_or(value_rows.len() + 1);
            let date_start = table_int(&source.matrix, "date_column_start").ok_or_else(|| {
                config_error(format!(
                    "{}: matrix.date_column_start mancante o non valido",
                    source.raw_path
                ))
            })?;
            let value_column = table_str(&source.matrix, "value_column").unwrap_or("value");
            let date_column = table_str(&source.matrix, "date_column").unwrap_or("Data");
            let label_column = table_str(&source.matrix, "label_column").unwrap_or("label");
            let mut previous_day = None;
            let mut output = Vec::new();
            let day_pattern = Regex::new(r"\b(\d{1,2})\b").expect("regex costante");
            for block_start in (0..rows.len()).step_by(block_height.max(1)) {
                let header = &rows[block_start];
                let mut dates = Vec::new();
                for (column, cell) in header.iter().enumerate().skip(date_start.saturating_sub(1)) {
                    let Some(day) = cell_text(cell).and_then(|value| {
                        day_pattern
                            .captures(&value)?
                            .get(1)?
                            .as_str()
                            .parse::<u32>()
                            .ok()
                    }) else {
                        continue;
                    };
                    if !(1..=31).contains(&day) {
                        continue;
                    }
                    if previous_day.is_some_and(|previous| day < previous) {
                        month += 1;
                    }
                    previous_day = Some(day);
                    let date = NaiveDate::from_ymd_opt(year as i32, month as u32, day).ok_or_else(
                        || {
                            config_error(format!(
                                "{}: data matrice non valida: {year}-{month}-{day}",
                                source.raw_path
                            ))
                        },
                    )?;
                    dates.push((column, date));
                }
                for (offset, label) in value_rows.iter().enumerate() {
                    let values = rows.get(block_start + offset + 1);
                    for (column, date) in &dates {
                        let mut row = Row::new();
                        row.insert(date_column.to_owned(), Cell::Date(date.to_string()));
                        row.insert(label_column.to_owned(), Cell::Text(label.clone()));
                        row.insert(
                            value_column.to_owned(),
                            values
                                .and_then(|row| row.get(*column))
                                .cloned()
                                .unwrap_or(Cell::Empty),
                        );
                        output.push(row);
                    }
                }
            }
            Ok(output)
        },
        trace_operation_result,
    )
}

fn points_from_rows(
    source: &SourceSpec,
    rows: &[Row],
    source_index: usize,
) -> Result<Vec<ObservationPoint>, OperationError> {
    trace_observation_call(
        "observation_imports::points_from_rows",
        || {
            vec![
                TraceField::new("source_path", TraceValue::path(&source.path)),
                TraceField::new("source", trace_sensitive_text(&format!("{source:#?}"))),
                TraceField::new("rows", trace_rows(rows)),
                TraceField::new("source_index", TraceValue::data(source_index as u64)),
            ]
        },
        || {
            let mut output = Vec::new();
            let mut previous_values: HashMap<(usize, String), Cell> = HashMap::new();
            for (row_index, row) in rows.iter().enumerate() {
                for (extract_index, extract) in source.extracts.iter().enumerate() {
                    let raw_date = mapped_value(
                        source,
                        extract,
                        row,
                        "date",
                        extract_index,
                        &mut previous_values,
                    );
                    let (date, date_note) =
                        match normalize_date(raw_date.as_ref(), &extract.date_policy) {
                            Ok(value) => value,
                            Err(_) if extract.skip_invalid_dates => continue,
                            Err(error) => return Err(error),
                        };
                    let Some(observation_date) = date else {
                        continue;
                    };
                    let compound = compound_values(source, extract, row)?;
                    let mut numeric_value = None;
                    let mut systolic = None;
                    let mut diastolic = None;
                    let mut pulse = None;
                    for (field, target) in [
                        ("numeric_value", &mut numeric_value),
                        ("systolic", &mut systolic),
                        ("diastolic", &mut diastolic),
                        ("pulse", &mut pulse),
                    ] {
                        let raw = mapped_value(
                            source,
                            extract,
                            row,
                            field,
                            extract_index,
                            &mut previous_values,
                        );
                        *target = number_value(raw.as_ref().or_else(|| compound.get(field)))?;
                    }
                    let text_cell = mapped_value(
                        source,
                        extract,
                        row,
                        "text_value",
                        extract_index,
                        &mut previous_values,
                    )
                    .or_else(|| {
                        mapped_value(
                            source,
                            extract,
                            row,
                            "value",
                            extract_index,
                            &mut previous_values,
                        )
                    })
                    .or_else(|| compound.get("text_value").cloned())
                    .or_else(|| compound.get("value").cloned());
                    let text_value = text_cell
                        .as_ref()
                        .and_then(cell_text)
                        .filter(|value| !value.is_empty());
                    if numeric_value.is_none()
                        && systolic.is_none()
                        && diastolic.is_none()
                        && text_value.is_none()
                    {
                        continue;
                    }
                    let mut notes = extract
                        .note_columns
                        .iter()
                        .filter_map(|column| row.get(column).and_then(cell_text))
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>();
                    if let Some(column) = extract.columns.get("note")
                        && let Some(value) = row
                            .get(column)
                            .and_then(cell_text)
                            .filter(|value| !value.is_empty())
                    {
                        notes.push(value);
                    }
                    if let Some(note) = &extract.static_note {
                        notes.push(note.clone());
                    }
                    if let Some(note) = date_note {
                        notes.push(note);
                    }
                    let series_slug = slug(&extract.series_id);
                    let series_slug = if series_slug.is_empty() {
                        "osservazione"
                    } else {
                        &series_slug
                    };
                    let id = format!(
                        "{}-{}-{}-{}-{}",
                        series_slug,
                        observation_date,
                        source_index + 1,
                        extract_index + 1,
                        row_index + 2
                    );
                    output.push(ObservationPoint {
                        id,
                        series_id: extract.series_id.clone(),
                        observation_date,
                        source_type: "spreadsheet".into(),
                        source_reference: extract.source_reference.clone(),
                        numeric_value,
                        text_value,
                        systolic,
                        diastolic,
                        pulse,
                        note: (!notes.is_empty()).then(|| notes.join(&extract.note_join)),
                        source_kind: "observation-import".into(),
                        document_id: None,
                        document_href: None,
                        document_title: None,
                        document_category: None,
                        source_text_digest: None,
                        original_line: None,
                        line_number: None,
                        page_number: None,
                        character_start: None,
                        character_end: None,
                        matched_label: None,
                        raw_value: None,
                        parsed_value: None,
                        raw_unit: None,
                        normalized_unit: None,
                        qualifier: None,
                        rule_id: None,
                        rule_version: None,
                        rule_digest: None,
                        reason_code: None,
                    });
                }
            }
            Ok(output)
        },
        trace_points_result,
    )
}

fn mapped_value(
    source: &SourceSpec,
    extract: &ExtractSpec,
    row: &Row,
    field: &str,
    extract_index: usize,
    previous: &mut HashMap<(usize, String), Cell>,
) -> Option<Cell> {
    trace_observation_call(
        "observation_imports::mapped_value",
        || {
            vec![
                TraceField::new("source_path", TraceValue::path(&source.path)),
                TraceField::new("field", trace_sensitive_text(field)),
                TraceField::new("extract_index", TraceValue::data(extract_index as u64)),
                TraceField::new("row", trace_rows(std::slice::from_ref(row))),
            ]
        },
        || {
            let column = extract.columns.get(field)?;
            let value = row.get(column).cloned().unwrap_or(Cell::Empty);
            let key = (extract_index, field.to_owned());
            if value == Cell::Empty && source.fill_down.contains(field) {
                return previous.get(&key).cloned();
            }
            if value != Cell::Empty {
                previous.insert(key, value.clone());
            }
            Some(value)
        },
        |result| {
            let source_column = extract.columns.get(field).map_or(field, String::as_str);
            trace_cell(source_column, result.as_ref())
        },
    )
}

fn compound_values(
    source: &SourceSpec,
    extract: &ExtractSpec,
    row: &Row,
) -> Result<HashMap<String, Cell>, OperationError> {
    trace_observation_call(
        "observation_imports::compound_values",
        || {
            let mut fields = vec![
                TraceField::new("source_path", TraceValue::path(&source.path)),
                TraceField::new("row", trace_rows(std::slice::from_ref(row))),
            ];
            if let Some(column) = &extract.compound_column {
                fields.push(TraceField::new(
                    "compound_column",
                    trace_sensitive_text(column.as_str()),
                ));
            }
            fields
        },
        || {
            let (Some(column), Some(pattern)) =
                (&extract.compound_column, &extract.compound_pattern)
            else {
                return Ok(HashMap::new());
            };
            let Some(raw) = row
                .get(column)
                .and_then(cell_text)
                .filter(|value| !value.is_empty())
            else {
                return Ok(HashMap::new());
            };
            let Some(captures) = pattern.captures(&raw) else {
                return Err(config_error(format!(
                    "{}: valore composto non valido per {column}: {raw}",
                    source.raw_path
                )));
            };
            Ok(pattern
                .capture_names()
                .flatten()
                .filter_map(|name| {
                    captures
                        .name(name)
                        .map(|value| (name.to_owned(), Cell::Text(value.as_str().to_owned())))
                })
                .collect())
        },
        trace_operation_result,
    )
}

fn normalize_date(
    value: Option<&Cell>,
    policy: &str,
) -> Result<(Option<String>, Option<String>), OperationError> {
    trace_observation_call(
        "observation_imports::normalize_date",
        || {
            vec![
                TraceField::new(
                    "value_type",
                    TraceValue::data(value.map_or("missing", |cell| match cell {
                        Cell::Empty => "empty",
                        Cell::Text(_) => "text",
                        Cell::Number(_) => "number",
                        Cell::Date(_) => "date",
                        Cell::Boolean(_) => "boolean",
                    })),
                ),
                TraceField::new("source_cell_value", trace_unmapped_cells_omission()),
                TraceField::new("policy", trace_sensitive_text(policy)),
            ]
        },
        || normalize_date_inner(value, policy),
        trace_operation_result,
    )
}

fn normalize_date_inner(
    value: Option<&Cell>,
    policy: &str,
) -> Result<(Option<String>, Option<String>), OperationError> {
    trace_observation_call(
        "observation_imports::normalize_date_inner",
        || {
            vec![
                TraceField::new(
                    "value_type",
                    TraceValue::data(value.map_or("missing", |cell| match cell {
                        Cell::Empty => "empty",
                        Cell::Text(_) => "text",
                        Cell::Number(_) => "number",
                        Cell::Date(_) => "date",
                        Cell::Boolean(_) => "boolean",
                    })),
                ),
                TraceField::new("source_cell_value", trace_unmapped_cells_omission()),
                TraceField::new("policy", trace_sensitive_text(policy)),
            ]
        },
        || {
            if policy == "exact" {
                let Some(value) = value.filter(|value| *value != &Cell::Empty) else {
                    return Ok((None, None));
                };
                return date_value(value)
                    .map(|date| (Some(date), None))
                    .ok_or_else(|| {
                        config_error(format!(
                            "data osservazione non valida: {}",
                            cell_text(value).unwrap_or_default()
                        ))
                    });
            }
            let Some(text) = value.and_then(cell_text).filter(|value| !value.is_empty()) else {
                return Ok((None, None));
            };
            let year = Regex::new(r"\b(19\d{2}|20\d{2})\b")
                .expect("regex costante")
                .captures(&text)
                .and_then(|captures| captures.get(1))
                .and_then(|value| value.as_str().parse::<i32>().ok())
                .ok_or_else(|| config_error(format!("anno osservazione non valido: {text}")))?;
            let note = match policy {
                "year_start" => format!("data approssimata: solo anno nel foglio ({text})"),
                "period_start" => format!("data approssimata: inizio periodo nel foglio ({text})"),
                _ => {
                    return Err(config_error(format!(
                        "politica data osservazione non supportata: {policy}"
                    )));
                }
            };
            Ok((Some(format!("{year:04}-01-01")), Some(note)))
        },
        trace_operation_result,
    )
}

fn date_value(value: &Cell) -> Option<String> {
    trace_observation_call(
        "observation_imports::date_value",
        || vec![TraceField::new("value", trace_cell("value", Some(value)))],
        || match value {
            Cell::Date(value) => Some(value.clone()),
            Cell::Text(value) => {
                let value = value.trim();
                ["%Y-%m-%d", "%d/%m/%Y", "%d-%m-%Y"]
                    .iter()
                    .find_map(|format| {
                        NaiveDate::parse_from_str(value, format)
                            .ok()
                            .map(|date| date.to_string())
                    })
            }
            _ => None,
        },
        |result| trace_sensitive_text(&format!("{result:#?}")),
    )
}

fn number_value(value: Option<&Cell>) -> Result<Option<f64>, OperationError> {
    trace_observation_call(
        "observation_imports::number_value",
        || {
            vec![
                TraceField::new(
                    "cell_type",
                    TraceValue::data(value.map_or("missing", |cell| match cell {
                        Cell::Empty => "empty",
                        Cell::Text(_) => "text",
                        Cell::Number(_) => "number",
                        Cell::Date(_) => "date",
                        Cell::Boolean(_) => "boolean",
                    })),
                ),
                TraceField::new("source_cell_value", trace_unmapped_cells_omission()),
            ]
        },
        || number_value_inner(value),
        trace_operation_result,
    )
}

fn number_value_inner(value: Option<&Cell>) -> Result<Option<f64>, OperationError> {
    trace_observation_call(
        "observation_imports::number_value_inner",
        || {
            vec![
                TraceField::new(
                    "cell_type",
                    TraceValue::data(value.map_or("missing", |cell| match cell {
                        Cell::Empty => "empty",
                        Cell::Text(_) => "text",
                        Cell::Number(_) => "number",
                        Cell::Date(_) => "date",
                        Cell::Boolean(_) => "boolean",
                    })),
                ),
                TraceField::new("source_cell_value", trace_unmapped_cells_omission()),
            ]
        },
        || {
            let Some(value) = value else {
                return Ok(None);
            };
            match value {
                Cell::Empty | Cell::Boolean(_) => Ok(None),
                Cell::Number(value) if value.is_finite() => Ok(Some(*value)),
                Cell::Number(value) => Err(config_error(format!(
                    "valore numerico osservazione non finito: {value}"
                ))),
                Cell::Text(value) => {
                    let normalized = value.trim().replace(',', ".");
                    if normalized.is_empty() {
                        return Ok(None);
                    }
                    normalized.parse::<f64>().map(Some).map_err(|_| {
                        config_error(format!("valore numerico osservazione non valido: {value}"))
                    })
                }
                Cell::Date(value) => value.parse::<f64>().map(Some).map_err(|_| {
                    config_error(format!("valore numerico osservazione non valido: {value}"))
                }),
            }
        },
        trace_operation_result,
    )
}

fn duplicate_warnings(series: &ObservationSeries, points: &[ObservationPoint]) -> Vec<String> {
    trace_observation_call(
        "observation_imports::duplicate_warnings",
        || {
            vec![
                TraceField::new("series_id", trace_sensitive_text(series.id.as_str())),
                TraceField::new(
                    "point_dates",
                    TraceValue::data(
                        points
                            .iter()
                            .filter(|point| point.series_id == series.id)
                            .map(|point| point.observation_date.as_str())
                            .collect::<Vec<_>>(),
                    ),
                ),
                TraceField::new("point_content", trace_unmapped_cells_omission()),
            ]
        },
        || duplicate_warnings_inner(series, points),
        |warnings| trace_sensitive_text(&format!("{warnings:#?}")),
    )
}

fn duplicate_warnings_inner(
    series: &ObservationSeries,
    points: &[ObservationPoint],
) -> Vec<String> {
    trace_observation_call(
        "observation_imports::duplicate_warnings_inner",
        || {
            vec![
                TraceField::new("series", trace_sensitive_text(&format!("{series:#?}"))),
                TraceField::new("point_count", TraceValue::data(points.len() as u64)),
                TraceField::new("point_content", trace_unmapped_cells_omission()),
            ]
        },
        || {
            let mut counts = BTreeMap::<&str, usize>::new();
            for point in points.iter().filter(|point| point.series_id == series.id) {
                *counts.entry(&point.observation_date).or_default() += 1;
            }
            counts
                .into_iter()
                .filter(|(_, count)| *count > 1)
                .map(|(day, count)| {
                    format!(
                        "serie {}: {count} misurazioni nella stessa data {day}",
                        series.id
                    )
                })
                .collect()
        },
        |warnings| trace_sensitive_text(&format!("{warnings:#?}")),
    )
}

fn write_outputs(
    metadata: &Path,
    result: &ObservationImportResult,
    manifest_text: &str,
    hashes: &BTreeMap<String, String>,
) -> Result<(), OperationError> {
    trace_observation_call(
        "observation_imports::write_outputs",
        || {
            let manifest = toml::from_str::<Value>(manifest_text)
                .map(|value| trace_toml_value(&value))
                .unwrap_or_else(|_| trace_omission("manifest_not_parseable"));
            vec![
                TraceField::new("metadata_path", TraceValue::path(metadata)),
                TraceField::new("manifest", manifest),
                TraceField::new(
                    "result",
                    TraceValue::record(&[
                        TraceField::new(
                            "series_count",
                            TraceValue::data(result.series.len() as u64),
                        ),
                        TraceField::new(
                            "point_count",
                            TraceValue::data(result.points.len() as u64),
                        ),
                    ]),
                ),
                TraceField::new(
                    "source_hashes",
                    trace_sensitive_text(&format!("{hashes:#?}")),
                ),
            ]
        },
        || write_outputs_inner(metadata, result, manifest_text, hashes),
        trace_operation_result,
    )
}

fn write_outputs_inner(
    metadata: &Path,
    result: &ObservationImportResult,
    manifest_text: &str,
    hashes: &BTreeMap<String, String>,
) -> Result<(), OperationError> {
    trace_observation_call(
        "observation_imports::write_outputs_inner",
        || {
            let manifest = toml::from_str::<Value>(manifest_text)
                .map(|value| trace_toml_value(&value))
                .unwrap_or_else(|_| trace_omission("manifest_not_parseable"));
            vec![
                TraceField::new("metadata_path", TraceValue::path(metadata)),
                TraceField::new("manifest", manifest),
                TraceField::new("series_count", TraceValue::data(result.series.len() as u64)),
                TraceField::new("point_count", TraceValue::data(result.points.len() as u64)),
                TraceField::new(
                    "source_hashes",
                    trace_sensitive_text(&format!("{hashes:#?}")),
                ),
            ]
        },
        || {
            let observations_dir = metadata.join("observations");
            if fs::symlink_metadata(&observations_dir)
                .is_ok_and(|entry| entry.file_type().is_symlink())
            {
                return Err(config_error(format!(
                    "la directory osservazioni non può essere un collegamento simbolico: {}",
                    observations_dir.display()
                )));
            }
            let directory = metadata.join(OUTPUT_DIR);
            if fs::symlink_metadata(&directory).is_ok_and(|entry| entry.file_type().is_symlink()) {
                return Err(config_error(format!(
                    "la directory di output importato non può essere un collegamento simbolico: {}",
                    directory.display()
                )));
            }
            if directory.exists() && !directory.is_dir() {
                return Err(config_error(format!(
                    "output import osservazioni non è una directory: {}",
                    directory.display()
                )));
            }
            fs::create_dir_all(&directory).map_err(|error| {
                config_error(format!(
                    "impossibile creare {}: {error}",
                    directory.display()
                ))
            })?;
            let series = series_value(&result.series);
            let points = points_value(&result.points);
            let series_text = toml::to_string(&series).map_err(|error| {
                config_error(format!(
                    "serializzazione serie osservazioni non riuscita: {error}"
                ))
            })?;
            let points_text = toml::to_string(&points).map_err(|error| {
                config_error(format!(
                    "serializzazione punti osservazioni non riuscita: {error}"
                ))
            })?;
            atomic_write(&directory.join("series.toml"), &series_text)?;
            atomic_write(&directory.join("points.toml"), &points_text)?;
            let mut state = format!(
                "manifest_hash = \"{}\"\n\n[source_hashes]\n",
                sha256(manifest_text.as_bytes())
            );
            for (path, hash) in hashes {
                state.push_str(&format!("{} = \"{}\"\n", toml_key(path), hash));
            }
            atomic_write(&directory.join("import_state.toml"), &state)
        },
        trace_operation_result,
    )
}

fn series_value(series: &[ObservationSeries]) -> Value {
    trace_observation_call(
        "observation_imports::series_value",
        || {
            vec![TraceField::new(
                "series",
                trace_sensitive_text(&format!("{series:#?}")),
            )]
        },
        || series_value_inner(series),
        trace_toml_value,
    )
}

fn series_value_inner(series: &[ObservationSeries]) -> Value {
    trace_observation_call(
        "observation_imports::series_value_inner",
        || {
            vec![TraceField::new(
                "series",
                trace_sensitive_text(&format!("{series:#?}")),
            )]
        },
        || {
            let values = series
                .iter()
                .map(|series| {
                    let mut table = Table::new();
                    table.insert("id".into(), Value::String(series.id.clone()));
                    table.insert("name".into(), Value::String(series.name.clone()));
                    table.insert(
                        "value_type".into(),
                        Value::String(series.value_type.clone()),
                    );
                    table.insert(
                        "warn_duplicate_same_day".into(),
                        Value::Boolean(series.warn_duplicate_same_day),
                    );
                    if let Some(value) = &series.unit {
                        table.insert("unit".into(), Value::String(value.clone()));
                    }
                    if let Some(value) = &series.description {
                        table.insert("description".into(), Value::String(value.clone()));
                    }
                    Value::Table(table)
                })
                .collect();
            let mut root = Table::new();
            root.insert("series".into(), Value::Array(values));
            Value::Table(root)
        },
        trace_toml_value,
    )
}

fn points_value(points: &[ObservationPoint]) -> Value {
    trace_observation_call(
        "observation_imports::points_value",
        || {
            vec![
                TraceField::new("point_count", TraceValue::data(points.len() as u64)),
                // A point no longer retains its originating column name for redaction.
                TraceField::new("content", trace_unmapped_cells_omission()),
            ]
        },
        || points_value_inner(points),
        |value| {
            let count = value
                .get("point")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            TraceValue::record(&[
                TraceField::new("point_count", TraceValue::data(count as u64)),
                TraceField::new("content", trace_unmapped_cells_omission()),
            ])
        },
    )
}

fn points_value_inner(points: &[ObservationPoint]) -> Value {
    trace_observation_call(
        "observation_imports::points_value_inner",
        || {
            vec![
                TraceField::new("point_count", TraceValue::data(points.len() as u64)),
                TraceField::new("content", trace_unmapped_cells_omission()),
            ]
        },
        || {
            let values = points
                .iter()
                .map(|point| {
                    let mut table = Table::new();
                    for (key, value) in [
                        ("id", &point.id),
                        ("series_id", &point.series_id),
                        ("observation_date", &point.observation_date),
                        ("source_type", &point.source_type),
                        ("source_reference", &point.source_reference),
                    ] {
                        table.insert(key.into(), Value::String(value.clone()));
                    }
                    for (key, value) in [
                        ("numeric_value", point.numeric_value),
                        ("systolic", point.systolic),
                        ("diastolic", point.diastolic),
                        ("pulse", point.pulse),
                    ] {
                        if let Some(value) = value {
                            table.insert(key.into(), Value::Float(value));
                        }
                    }
                    if let Some(value) = &point.text_value {
                        table.insert("text_value".into(), Value::String(value.clone()));
                    }
                    if let Some(value) = &point.note {
                        table.insert("note".into(), Value::String(value.clone()));
                    }
                    Value::Table(table)
                })
                .collect();
            let mut root = Table::new();
            root.insert("point".into(), Value::Array(values));
            Value::Table(root)
        },
        |value| {
            let count = value
                .get("point")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            TraceValue::record(&[
                TraceField::new("point_count", TraceValue::data(count as u64)),
                TraceField::new("content", trace_unmapped_cells_omission()),
            ])
        },
    )
}

fn atomic_write(path: &Path, content: &str) -> Result<(), OperationError> {
    trace_observation_call(
        "observation_imports::atomic_write",
        || {
            vec![
                TraceField::new("path", TraceValue::path(path)),
                // Source column provenance has been lost at this serialized boundary.
                TraceField::new("content", trace_unmapped_cells_omission()),
            ]
        },
        || atomic_write_inner(path, content),
        trace_operation_result,
    )
}

fn atomic_write_inner(path: &Path, content: &str) -> Result<(), OperationError> {
    trace_observation_call(
        "observation_imports::atomic_write_inner",
        || {
            vec![
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("content", trace_unmapped_cells_omission()),
            ]
        },
        || {
            let temp = path.with_extension(format!("tmp-{}", std::process::id()));
            fs::write(&temp, content).map_err(|error| {
                config_error(format!(
                    "scrittura {} non riuscita: {error}",
                    temp.display()
                ))
            })?;
            fs::rename(&temp, path).map_err(|error| {
                config_error(format!(
                    "aggiornamento {} non riuscito: {error}",
                    path.display()
                ))
            })
        },
        trace_operation_result,
    )
}

fn hash_sources(sources: &[SourceSpec]) -> Result<BTreeMap<String, String>, OperationError> {
    trace_observation_call(
        "observation_imports::hash_sources",
        || {
            vec![TraceField::new(
                "sources",
                trace_sensitive_text(&format!("{sources:#?}")),
            )]
        },
        || hash_sources_inner(sources),
        trace_operation_result,
    )
}

fn hash_sources_inner(sources: &[SourceSpec]) -> Result<BTreeMap<String, String>, OperationError> {
    trace_observation_call(
        "observation_imports::hash_sources_inner",
        || {
            vec![
                TraceField::new("sources", trace_sensitive_text(&format!("{sources:#?}"))),
                TraceField::new(
                    "source_bytes",
                    trace_omission("raw_source_bytes_not_recorded"),
                ),
            ]
        },
        || {
            sources
                .iter()
                .map(|source| {
                    fs::read(&source.path)
                        .map(|bytes| (source.raw_path.clone(), sha256(&bytes)))
                        .map_err(|error| {
                            config_error(format!(
                                "lettura sorgente {} non riuscita: {error}",
                                source.path.display()
                            ))
                        })
                })
                .collect()
        },
        |result| match result {
            Ok(hashes) => trace_sensitive_text(&format!("{hashes:#?}")),
            Err(error) => trace_sensitive_text(&format!("{error:#?}")),
        },
    )
}

fn sha256(value: &[u8]) -> String {
    trace_observation_call(
        "observation_imports::sha256",
        || {
            vec![
                TraceField::new("byte_length", TraceValue::data(value.len() as u64)),
                TraceField::new("raw_bytes", trace_omission("raw_source_bytes_not_recorded")),
            ]
        },
        || format!("{:x}", Sha256::digest(value)),
        |digest| TraceValue::data(digest.as_str()),
    )
}

fn read_text(path: &Path) -> Result<String, OperationError> {
    trace_observation_call(
        "observation_imports::read_text",
        || {
            vec![
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new(
                    "raw_text",
                    trace_omission("text_is_recorded_after_structured_parsing"),
                ),
            ]
        },
        || {
            fs::read_to_string(path).map_err(|error| {
                config_error(format!("lettura {} non riuscita: {error}", path.display()))
            })
        },
        |result| match result {
            Ok(text) => TraceValue::record(&[
                TraceField::new("state", TraceValue::data("ok")),
                TraceField::new("byte_length", TraceValue::data(text.len() as u64)),
            ]),
            Err(error) => trace_error(error),
        },
    )
}

fn cell_text(cell: &Cell) -> Option<String> {
    trace_observation_call(
        "observation_imports::cell_text",
        || {
            vec![
                TraceField::new(
                    "cell_type",
                    TraceValue::data(match cell {
                        Cell::Empty => "empty",
                        Cell::Text(_) => "text",
                        Cell::Number(_) => "number",
                        Cell::Date(_) => "date",
                        Cell::Boolean(_) => "boolean",
                    }),
                ),
                TraceField::new("cell_value", trace_unmapped_cells_omission()),
            ]
        },
        || match cell {
            Cell::Empty => None,
            Cell::Text(value) => Some(value.trim().to_owned()),
            Cell::Number(value) => Some(value.to_string()),
            Cell::Date(value) => Some(value.clone()),
            Cell::Boolean(value) => Some(value.to_string()),
        },
        |result| {
            TraceValue::record(&[
                TraceField::new("has_value", TraceValue::data(result.is_some())),
                TraceField::new("cell_value", trace_unmapped_cells_omission()),
            ])
        },
    )
}
fn slug(value: &str) -> String {
    trace_observation_call(
        "observation_imports::slug",
        || vec![TraceField::new("value", trace_sensitive_text(value))],
        || {
            let slug: String = value
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || "_.-".contains(ch) {
                        ch
                    } else {
                        '-'
                    }
                })
                .collect();
            slug.trim_matches('-').to_owned()
        },
        |result| trace_sensitive_text(result),
    )
}
fn toml_key(value: &str) -> String {
    trace_observation_call(
        "observation_imports::toml_key",
        || vec![TraceField::new("value", trace_sensitive_text(value))],
        || format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")),
        |result| trace_sensitive_text(result),
    )
}

fn array<'a>(table: &'a Table, key: &str, path: &Path) -> Result<&'a [Value], OperationError> {
    trace_observation_call(
        "observation_imports::array",
        || {
            vec![
                TraceField::new("table", trace_toml_table(table)),
                TraceField::new("key", trace_sensitive_text(key)),
                TraceField::new("path", TraceValue::path(path)),
            ]
        },
        || match table.get(key) {
            None => Ok(&[] as &[Value]),
            Some(Value::Array(values)) => Ok(values.as_slice()),
            Some(_) => Err(config_error(format!(
                "{}: campo {key} deve essere un array di tabelle",
                path.display()
            ))),
        },
        |result| match result {
            Ok(values) => trace_toml_value(&Value::Array(values.to_vec())),
            Err(error) => trace_error(error),
        },
    )
}
fn table<'a>(
    value: &'a Value,
    path: &Path,
    key: &str,
    index: usize,
) -> Result<&'a Table, OperationError> {
    trace_observation_call(
        "observation_imports::table",
        || {
            vec![
                TraceField::new("value", trace_toml_value(value)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("key", trace_sensitive_text(key)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || {
            value.as_table().ok_or_else(|| {
                config_error(format!(
                    "{}: elemento {index} di {key} deve essere una tabella",
                    path.display()
                ))
            })
        },
        |result| match result {
            Ok(table) => trace_toml_table(table),
            Err(error) => trace_error(error),
        },
    )
}
fn required_string(
    table: &Table,
    field: &str,
    path: &Path,
    index: usize,
) -> Result<String, OperationError> {
    trace_observation_call(
        "observation_imports::required_string",
        || {
            vec![
                TraceField::new("table", trace_toml_table(table)),
                TraceField::new("field", trace_sensitive_text(field)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || {
            table
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    config_error(format!(
                        "{}: elemento {index} campo {field} deve essere una stringa non vuota",
                        path.display()
                    ))
                })
        },
        |result| match result {
            Ok(value) => trace_sensitive_text(value),
            Err(error) => trace_error(error),
        },
    )
}
fn optional_string(
    table: &Table,
    field: &str,
    path: &Path,
) -> Result<Option<String>, OperationError> {
    trace_observation_call(
        "observation_imports::optional_string",
        || {
            vec![
                TraceField::new("table", trace_toml_table(table)),
                TraceField::new("field", trace_sensitive_text(field)),
                TraceField::new("path", TraceValue::path(path)),
            ]
        },
        || match table.get(field) {
            None => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.trim().to_owned())),
            Some(_) => Err(config_error(format!(
                "{}: campo {field} deve essere una stringa",
                path.display()
            ))),
        },
        |result| match result {
            Ok(value) => trace_sensitive_text(&format!("{value:#?}")),
            Err(error) => trace_error(error),
        },
    )
}
fn optional_bool(
    table: &Table,
    field: &str,
    default: bool,
    path: &Path,
    index: usize,
) -> Result<bool, OperationError> {
    trace_observation_call(
        "observation_imports::optional_bool",
        || {
            vec![
                TraceField::new("table", trace_toml_table(table)),
                TraceField::new("field", trace_sensitive_text(field)),
                TraceField::new("default", TraceValue::data(default)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || match table.get(field) {
            None => Ok(default),
            Some(Value::Boolean(value)) => Ok(*value),
            Some(_) => Err(config_error(format!(
                "{}: elemento {index} campo {field} deve essere booleano",
                path.display()
            ))),
        },
        trace_operation_result,
    )
}
fn optional_usize(
    table: &Table,
    field: &str,
    default: usize,
    path: &Path,
    index: usize,
) -> Result<usize, OperationError> {
    trace_observation_call(
        "observation_imports::optional_usize",
        || {
            vec![
                TraceField::new("table", trace_toml_table(table)),
                TraceField::new("field", trace_sensitive_text(field)),
                TraceField::new("default", TraceValue::data(default as u64)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || match table.get(field) {
            None => Ok(default),
            Some(value) => positive_usize(value, path, field, index),
        },
        trace_operation_result,
    )
}
fn optional_usize_opt(
    table: &Table,
    field: &str,
    path: &Path,
    index: usize,
) -> Result<Option<usize>, OperationError> {
    trace_observation_call(
        "observation_imports::optional_usize_opt",
        || {
            vec![
                TraceField::new("table", trace_toml_table(table)),
                TraceField::new("field", trace_sensitive_text(field)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || {
            table
                .get(field)
                .map(|value| positive_usize(value, path, field, index))
                .transpose()
        },
        trace_operation_result,
    )
}
fn positive_usize(
    value: &Value,
    path: &Path,
    field: &str,
    index: usize,
) -> Result<usize, OperationError> {
    trace_observation_call(
        "observation_imports::positive_usize",
        || {
            vec![
                TraceField::new("value", trace_toml_value(value)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("field", trace_sensitive_text(field)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || {
            value
                .as_integer()
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    config_error(format!(
                        "{}: elemento {index} campo {field} deve essere un intero positivo",
                        path.display()
                    ))
                })
        },
        trace_operation_result,
    )
}
fn table_int(table: &Table, field: &str) -> Option<usize> {
    trace_observation_call(
        "observation_imports::table_int",
        || {
            vec![
                TraceField::new("table", trace_toml_table(table)),
                TraceField::new("field", trace_sensitive_text(field)),
            ]
        },
        || {
            table
                .get(field)?
                .as_integer()
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
        },
        |value| trace_sensitive_text(&format!("{value:#?}")),
    )
}
fn table_str<'a>(table: &'a Table, field: &str) -> Option<&'a str> {
    trace_observation_call(
        "observation_imports::table_str",
        || {
            vec![
                TraceField::new("table", trace_toml_table(table)),
                TraceField::new("field", trace_sensitive_text(field)),
            ]
        },
        || table.get(field)?.as_str(),
        |value| trace_sensitive_text(&format!("{value:#?}")),
    )
}
fn string_array(
    value: Option<&Value>,
    path: &Path,
    field: &str,
    index: usize,
) -> Result<Vec<String>, OperationError> {
    trace_observation_call(
        "observation_imports::string_array",
        || {
            vec![
                TraceField::new("value", trace_named_toml_value(field, value)),
                TraceField::new("path", TraceValue::path(path)),
                TraceField::new("field", trace_sensitive_text(field)),
                TraceField::new("index", TraceValue::data(index as u64)),
            ]
        },
        || {
            let Some(value) = value else {
                return Ok(Vec::new());
            };
            let values = value.as_array().ok_or_else(|| {
                config_error(format!(
                    "{}: elemento {index} campo {field} deve essere un array di stringhe",
                    path.display()
                ))
            })?;
            values
                .iter()
                .map(|value| {
                    value.as_str().map(str::to_owned).ok_or_else(|| {
                        config_error(format!(
                            "{}: elemento {index} campo {field} deve contenere solo stringhe",
                            path.display()
                        ))
                    })
                })
                .collect()
        },
        |result| match result {
            Ok(values) => trace_sensitive_text(&format!("{values:#?}")),
            Err(error) => trace_error(error),
        },
    )
}
fn config_error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    let trace = trace_current(Component::Core, "observation_imports::config_error", || {
        vec![TraceField::new("message", trace_sensitive_text(&message))]
    });
    let error = OperationError::new(ErrorCode::Configuration, message);
    if let Some(trace) = trace {
        let _ = trace.finish(trace_error(&error));
    }
    error
}

#[cfg(test)]
mod tests;
