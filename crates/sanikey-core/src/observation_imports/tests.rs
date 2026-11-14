//! Fixture sintetiche per importazione e aggiornamento delle osservazioni.

use super::{Cell, ensure_observation_imports_current, import_observations};
use crate::logging::{Component, Level, Session};
use serde_json::{Value as JsonValue, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use toml::{Table, Value as TomlValue};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

/// Gestisce le directory metadata e documenti create dalle fixture.
impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sanikey-observation-import-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("directory test");
        Self(path)
    }

    fn roots(&self) -> (PathBuf, PathBuf) {
        let metadata = self.0.join("metadata");
        let documents = self.0.join("documents");
        fs::create_dir_all(&metadata).expect("directory metadata");
        fs::create_dir_all(&documents).expect("directory documenti");
        (metadata, documents)
    }
}

/// Rimuove le directory sintetiche dopo i test.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setup(csv: &str, manifest: &str) -> (TestDirectory, PathBuf, PathBuf) {
    let directory = TestDirectory::new();
    let (metadata, documents) = directory.roots();
    fs::write(documents.join("input.csv"), csv).expect("CSV sintetico");
    fs::write(metadata.join("observation_imports.toml"), manifest).expect("manifest sintetico");
    (directory, metadata, documents)
}

fn traced<R>(operation: impl FnOnce() -> R) -> (R, String, Vec<JsonValue>) {
    let directory = TestDirectory::new();
    let log_directory = directory.0.join("trace");
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");
    let result = session.scoped(operation);
    session.finish(Component::Core).expect("chiusura Trace");
    let contents = fs::read_to_string(log_path).expect("log Trace sintetico");
    let records = contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
        .collect();
    (result, contents, records)
}

fn function_span<'a>(records: &'a [JsonValue], function: &str) -> (&'a JsonValue, &'a JsonValue) {
    let enters: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(enters.len(), 1, "ingresso univoco di {function}");
    let enter = enters[0];
    let exits: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_exit"
                && record["function"] == function
                && record["call_id"] == enter["call_id"]
        })
        .collect();
    assert_eq!(exits.len(), 1, "uscita correlata di {function}");
    let exit = exits[0];
    assert_eq!(exit["outcome"], "returned");
    assert!(exit["duration_ms"].as_u64().is_some());
    (enter, exit)
}

fn expected_data(value: JsonValue) -> JsonValue {
    json!({"kind":"data", "value":value})
}

fn expected_text(value: &str) -> JsonValue {
    expected_data(json!({
        "format":"text_redacted_known_secret_assignments",
        "value":value
    }))
}

fn expected_omission(reason: &str) -> JsonValue {
    expected_data(json!([
        expected_argument("state", expected_data(json!("omitted"))),
        expected_argument("reason", expected_data(json!(reason)))
    ]))
}

fn expected_argument(name: &str, content: JsonValue) -> JsonValue {
    json!({"name":name, "content":content})
}

fn assert_span(records: &[JsonValue], function: &str, arguments: JsonValue, result: JsonValue) {
    let (enter, exit) = function_span(records, function);
    assert_eq!(enter["arguments"], arguments);
    assert_eq!(exit["result"], result);
}

#[test]
fn imports_normalized_points_without_overwriting_curated_metadata() {
    let (directory, metadata, documents) = setup(
        "Data,Peso,Note\n2026-01-02,70.5,prima misura\n",
        "[[series]]\nid='peso'\nname='Peso'\nvalue_type='numeric'\nunit='kg'\n\n[[source]]\npath='input.csv'\nseries_id='peso'\n[source.columns]\ndate='Data'\nnumeric_value='Peso'\nnote='Note'\n",
    );
    fs::create_dir_all(metadata.join("observations")).expect("directory osservazioni curate");
    fs::write(
        metadata.join("observations/series.toml"),
        "curated = true\n",
    )
    .expect("metadati curati sintetici");

    let imported = import_observations(&metadata, &documents).expect("importazione riuscita");

    assert_eq!(imported.series[0].name, "Peso");
    assert_eq!(imported.points.len(), 1);
    assert_eq!(imported.points[0].numeric_value, Some(70.5));
    assert_eq!(imported.points[0].note.as_deref(), Some("prima misura"));
    assert_eq!(imported.points[0].source_kind, "observation-import");
    assert_eq!(
        fs::read_to_string(metadata.join("observations/series.toml")).unwrap(),
        "curated = true\n"
    );
    ensure_observation_imports_current(&metadata, &documents).expect("output aggiornato");
    let saved: toml::Value = toml::from_str(
        &fs::read_to_string(metadata.join("observations/imported/points.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["point"][0]["numeric_value"].as_float(), Some(70.5));
    drop(directory);
}

#[test]
fn duplicate_warning_can_be_disabled() {
    let (directory, metadata, documents) = setup(
        "Data,Sistolica,Diastolica\n2026-01-02,120,80\n2026-01-02,125,82\n",
        "[[series]]\nid='pressione'\nname='Pressione'\nvalue_type='blood_pressure'\n\n[[source]]\npath='input.csv'\nseries_id='pressione'\n[source.columns]\ndate='Data'\nsystolic='Sistolica'\ndiastolic='Diastolica'\n",
    );
    let result = import_observations(&metadata, &documents).expect("importazione riuscita");
    assert_eq!(
        result.warnings,
        ["serie pressione: 2 misurazioni nella stessa data 2026-01-02"]
    );
    let manifest = fs::read_to_string(metadata.join("observation_imports.toml"))
        .unwrap()
        .replace(
            "value_type='blood_pressure'",
            "value_type='blood_pressure'\nwarn_duplicate_same_day=false",
        );
    fs::write(metadata.join("observation_imports.toml"), manifest).unwrap();
    let result = import_observations(&metadata, &documents).expect("reimportazione riuscita");
    assert!(result.warnings.is_empty());
    drop(directory);
}

#[test]
fn stale_source_changes_are_rejected() {
    let (_directory, metadata, documents) = setup(
        "Data,INR\n2026-01-02,2.1\n",
        "[[series]]\nid='inr'\nname='INR'\nvalue_type='numeric'\n\n[[source]]\npath='input.csv'\nseries_id='inr'\n[source.columns]\ndate='Data'\nnumeric_value='INR'\n",
    );
    import_observations(&metadata, &documents).expect("importazione riuscita");
    fs::write(documents.join("input.csv"), "Data,INR\n2026-01-02,2.4\n").unwrap();
    assert!(
        ensure_observation_imports_current(&metadata, &documents)
            .unwrap_err()
            .to_string()
            .contains("non aggiornate")
    );
}

#[test]
fn supports_dirty_headers_fill_down_and_notes() {
    let (_directory, metadata, documents) = setup(
        "Titolo diario,,,,\nData,Ora,Sistolica,Diastolica,Frequenza\n2026-01-02,08:00,120,80,60\n,21:00,125,82,64\n",
        "[[series]]\nid='pressione'\nname='Pressione'\nvalue_type='blood_pressure'\nwarn_duplicate_same_day=false\n\n[[source]]\npath='input.csv'\nseries_id='pressione'\nheader_row=2\nfill_down=['date']\n[source.columns]\ndate='Data'\nsystolic='Sistolica'\ndiastolic='Diastolica'\npulse='Frequenza'\nnote='Ora'\n",
    );
    let result = import_observations(&metadata, &documents).expect("importazione riuscita");
    assert_eq!(result.points.len(), 2);
    assert!(
        result
            .points
            .iter()
            .all(|point| point.observation_date == "2026-01-02")
    );
    assert_eq!(result.points[1].note.as_deref(), Some("21:00"));
}

#[test]
fn supports_multiple_extracts_and_approximate_year_dates() {
    let (_directory, metadata, documents) = setup(
        "Anno,Peso,BMI,Evento,Farmaci\n2025,141.2,46.58,dimissione,terapia invariata\n",
        "[[series]]\nid='peso'\nname='Peso'\nvalue_type='numeric'\nunit='kg'\n[[series]]\nid='bmi'\nname='BMI'\nvalue_type='numeric'\nunit='kg/m2'\n\n[[source]]\npath='input.csv'\n[[source.extract]]\nseries_id='peso'\ndate_policy='year_start'\nnote_columns=['Evento','Farmaci']\n[source.extract.columns]\ndate='Anno'\nnumeric_value='Peso'\n[[source.extract]]\nseries_id='bmi'\ndate_policy='year_start'\nstatic_note='BMI da foglio peso'\n[source.extract.columns]\ndate='Anno'\nnumeric_value='BMI'\n",
    );
    let result = import_observations(&metadata, &documents).expect("importazione riuscita");
    assert_eq!(result.series.len(), 2);
    assert_eq!(result.points.len(), 2);
    assert_eq!(result.points[0].observation_date, "2025-01-01");
    assert!(
        result.points.iter().all(|point| point
            .note
            .as_deref()
            .unwrap()
            .contains("data approssimata"))
    );
}

#[test]
fn parses_compound_blood_pressure_values() {
    let (_directory, metadata, documents) = setup(
        "Data,Misura\n2026-01-02,113/65 54\n",
        "[[series]]\nid='pressione'\nname='Pressione'\nvalue_type='blood_pressure'\n\n[[source]]\npath='input.csv'\nseries_id='pressione'\n[source.columns]\ndate='Data'\n[source.compound_value]\ncolumn='Misura'\npattern='^(?P<systolic>\\d+)/(?P<diastolic>\\d+)\\s+(?P<pulse>\\d+)$'\n",
    );
    let result = import_observations(&metadata, &documents).expect("importazione riuscita");
    assert_eq!(result.points[0].systolic, Some(113.0));
    assert_eq!(result.points[0].diastolic, Some(65.0));
    assert_eq!(result.points[0].pulse, Some(54.0));
}

#[test]
fn normalizes_repeating_matrix_with_month_rollover() {
    let (_directory, metadata, documents) = setup(
        ",Domenica 28,lunedi 29,martedi 30,mercoledi 1\nmattino,124/84 67,135/82 67,114/68 60,115/73 75\npomeriggio,106/61 67,104/60 57,108/63 62,111/75 93\n",
        "[[series]]\nid='pressione'\nname='Pressione'\nvalue_type='blood_pressure'\nwarn_duplicate_same_day=false\n\n[[source]]\npath='input.csv'\nlayout='repeating_matrix'\n[source.matrix]\nyear=2008\nstart_month=9\nblock_height=3\ndate_column_start=2\nvalue_rows=['mattino','pomeriggio']\ndate_column='Data'\nvalue_column='Misura'\nlabel_column='Fascia'\n[[source.extract]]\nseries_id='pressione'\nnote_columns=['Fascia']\n[source.extract.columns]\ndate='Data'\n[source.extract.compound_value]\ncolumn='Misura'\npattern='^(?P<systolic>\\d+)/(?P<diastolic>\\d+)\\s+(?P<pulse>\\d+)$'\n",
    );
    let result = import_observations(&metadata, &documents).expect("importazione riuscita");
    assert_eq!(result.points.len(), 8);
    assert_eq!(result.points[0].observation_date, "2008-09-28");
    assert_eq!(result.points[7].observation_date, "2008-10-01");
    assert_eq!(result.points[7].note.as_deref(), Some("pomeriggio"));
}

#[test]
fn supports_xlsx_spreadsheets() {
    let directory = TestDirectory::new();
    let (metadata, documents) = directory.roots();
    write_xlsx(&documents.join("input.xlsx"));
    fs::write(metadata.join("observation_imports.toml"), "[[series]]\nid='peso'\nname='Peso'\nvalue_type='numeric'\nunit='kg'\n[[source]]\npath='input.xlsx'\nseries_id='peso'\n[source.columns]\ndate='Data'\nnumeric_value='Peso'\n").unwrap();
    let result = import_observations(&metadata, &documents).expect("lettura XLSX");
    assert_eq!(result.points[0].observation_date, "2026-01-02");
    assert_eq!(result.points[0].numeric_value, Some(70.5));
}

#[test]
fn trace_records_private_import_content_but_redacts_identified_secrets() {
    let secret = "synthetic-secret-observation-9274";
    let private_note = "nota privata sintetica controllo 4821";
    let (directory, metadata, documents) = setup(
        &format!("Data,Peso,Nota,api_key\n2026-01-02,70.5,{private_note},{secret}\n"),
        &format!(
            "[[series]]\nid='peso'\nname='Peso'\nvalue_type='numeric'\napi_key='{secret}'\n\n[[source]]\npath='input.csv'\nseries_id='peso'\n[source.columns]\ndate='Data'\nnumeric_value='Peso'\nnote='api_key'\n"
        ),
    );
    let log_directory = directory.0.join("trace-log");
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");

    let imported = session
        .scoped(|| import_observations(&metadata, &documents))
        .expect("importazione sintetica");
    assert_eq!(imported.points[0].note.as_deref(), Some(secret));
    drop(session);

    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(log.contains(private_note));
    assert!(log.contains("secret_omitted"));
    for function in [
        "observation_imports::csv_cell",
        "observation_imports::headers_from_rows",
        "observation_imports::mapped_value",
        "observation_imports::compound_values",
        "observation_imports::cell_text",
        "observation_imports::array",
        "observation_imports::required_string",
        "observation_imports::write_outputs_inner",
        "observation_imports::atomic_write_inner",
    ] {
        assert!(log.contains(function), "missing trace span {function}");
    }
    assert!(
        !log.contains(secret),
        "secret escaped through trace record(s): {}",
        log.lines()
            .filter(|line| line.contains(secret))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn events_level_does_not_record_private_import_cells() {
    let private_note = "nota privata sintetica controllo 6382";
    let (_directory, metadata, documents) = setup(
        &format!("Data,Nota\n2026-01-02,{private_note}\n"),
        "[[series]]\nid='peso'\nname='Peso'\nvalue_type='numeric'\n\n[[source]]\npath='input.csv'\nseries_id='peso'\n[source.columns]\ndate='Data'\nnote='Nota'\n",
    );
    let log_directory = metadata.join("event-log");
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Events)
            .expect("sessione Events sintetica");

    session
        .scoped(|| import_observations(&metadata, &documents))
        .expect("importazione sintetica");
    drop(session);

    let log = fs::read_to_string(log_path).expect("log Events sintetico");
    assert!(!log.contains(private_note));
}

#[test]
fn error_text_is_redacted_in_trace_without_changing_the_returned_error() {
    let secret = "synthetic-secret-error-3817";
    let policy = format!("password={secret}");
    let (directory, metadata, documents) = setup(
        "Anno,Peso\n2025,70.5\n",
        &format!(
            "[[series]]\nid='peso'\nname='Peso'\nvalue_type='numeric'\n\n[[source]]\npath='input.csv'\n[[source.extract]]\nseries_id='peso'\ndate_policy='{policy}'\n[source.extract.columns]\ndate='Anno'\nnumeric_value='Peso'\n"
        ),
    );
    let log_directory = directory.0.join("error-log");
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");

    let error = session
        .scoped(|| import_observations(&metadata, &documents))
        .expect_err("policy data non supportata");
    assert!(error.message().contains(&policy));
    drop(session);

    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(
        !log.contains(secret),
        "secret escaped through trace record(s): {}",
        log.lines()
            .filter(|line| line.contains(secret))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(log.contains("secret_omitted"));
}

#[test]
fn synthetic_trace_records_complete_observation_import_helper_spans() {
    let slug_input = "Hb / 13.7";
    let (slug, _, slug_records) = traced(|| super::slug(slug_input));
    assert_eq!(slug, "Hb---13.7");
    assert_span(
        &slug_records,
        "observation_imports::slug",
        json!([expected_argument("value", expected_text(slug_input))]),
        expected_text("Hb---13.7"),
    );

    let secret = "synthetic-observation-token-41";
    let key_input = format!("api_key={secret}");
    let expected_key = format!("\"api_key={secret}\"");
    let (key, key_log, key_records) = traced(|| super::toml_key(&key_input));
    assert_eq!(key, expected_key);
    assert!(key_log.contains("secret_omitted"));
    assert!(!key_log.contains(secret));
    assert_span(
        &key_records,
        "observation_imports::toml_key",
        json!([expected_argument(
            "value",
            expected_text("api_key=[secret_omitted]")
        )]),
        expected_text("\"api_key=[secret_omitted]\""),
    );

    let date_cell = Cell::Date("2026-05-06".into());
    let (date, _, date_records) = traced(|| super::date_value(&date_cell));
    assert_eq!(date.as_deref(), Some("2026-05-06"));
    assert_span(
        &date_records,
        "observation_imports::date_value",
        json!([expected_argument(
            "value",
            expected_data(json!({"value":{"type":"string", "value":"2026-05-06"}}))
        )]),
        expected_text("Some(\n    \"2026-05-06\",\n)"),
    );

    let private_cell = Cell::Text("nota sintetica non registrata".into());
    let (cell, _, cell_records) = traced(|| super::cell_text(&private_cell));
    assert_eq!(cell.as_deref(), Some("nota sintetica non registrata"));
    assert_span(
        &cell_records,
        "observation_imports::cell_text",
        json!([
            expected_argument("cell_type", expected_data(json!("text"))),
            expected_argument(
                "cell_value",
                expected_omission("source_column_provenance_unavailable")
            )
        ]),
        expected_data(json!([
            expected_argument("has_value", expected_data(json!(true))),
            expected_argument(
                "cell_value",
                expected_omission("source_column_provenance_unavailable")
            )
        ])),
    );

    let mut settings = Table::new();
    settings.insert(
        "api_key".into(),
        TomlValue::String("synthetic-key-58".into()),
    );
    settings.insert("limit".into(), TomlValue::Integer(7));
    let (limit, limit_log, limit_records) = traced(|| super::table_int(&settings, "limit"));
    assert_eq!(limit, Some(7));
    assert!(limit_log.contains("secret_omitted"));
    assert!(!limit_log.contains("synthetic-key-58"));
    assert_span(
        &limit_records,
        "observation_imports::table_int",
        json!([
            expected_argument(
                "table",
                expected_data(json!({
                    "api_key":{"kind":"secret_omitted"},
                    "limit":{"type":"integer", "value":7}
                }))
            ),
            expected_argument("field", expected_text("limit"))
        ]),
        expected_text("Some(\n    7,\n)"),
    );

    let mut labels = Table::new();
    labels.insert(
        "labels".into(),
        TomlValue::Array(vec![
            TomlValue::String("first".into()),
            TomlValue::String("second".into()),
        ]),
    );
    labels.insert(
        "api_key".into(),
        TomlValue::String("synthetic-array-key".into()),
    );
    let path = Path::new("/synthetic/observation-imports.toml");
    let (values, values_log, values_records) = traced(|| super::array(&labels, "labels", path));
    assert_eq!(
        values.expect("array value"),
        [
            TomlValue::String("first".into()),
            TomlValue::String("second".into())
        ]
    );
    assert!(values_log.contains("secret_omitted"));
    assert!(!values_log.contains("synthetic-array-key"));
    assert_span(
        &values_records,
        "observation_imports::array",
        json!([
            expected_argument(
                "table",
                expected_data(json!({
                    "api_key":{"kind":"secret_omitted"},
                    "labels":{"type":"array", "value":[
                        {"type":"string", "value":"first"},
                        {"type":"string", "value":"second"}
                    ]}
                }))
            ),
            expected_argument("key", expected_text("labels")),
            expected_argument(
                "path",
                expected_data(
                    json!({"encoding":"utf8", "value":"/synthetic/observation-imports.toml"})
                )
            )
        ]),
        expected_data(json!({
            "type":"array",
            "value":[
                {"type":"string", "value":"first"},
                {"type":"string", "value":"second"}
            ]
        })),
    );

    let mut text_config = Table::new();
    text_config.insert("title".into(), TomlValue::String("  sintesi  ".into()));
    let (title, _, title_records) = traced(|| super::table_str(&text_config, "title"));
    assert_eq!(title, Some("  sintesi  "));
    assert_span(
        &title_records,
        "observation_imports::table_str",
        json!([
            expected_argument(
                "table",
                expected_data(json!({"title":{"type":"string", "value":"  sintesi  "}}))
            ),
            expected_argument("field", expected_text("title"))
        ]),
        expected_text("Some(\n    \"  sintesi  \",\n)"),
    );

    let mut bool_config = Table::new();
    bool_config.insert("enabled".into(), TomlValue::Boolean(false));
    bool_config.insert(
        "token".into(),
        TomlValue::String("synthetic-bool-token".into()),
    );
    let bool_path = Path::new("/synthetic/observation-imports.toml");
    let (enabled, bool_log, bool_records) =
        traced(|| super::optional_bool(&bool_config, "enabled", true, bool_path, 4));
    assert!(!enabled.expect("boolean setting"));
    assert!(bool_log.contains("secret_omitted"));
    assert!(!bool_log.contains("synthetic-bool-token"));
    assert_span(
        &bool_records,
        "observation_imports::optional_bool",
        json!([
            expected_argument(
                "table",
                expected_data(json!({
                    "enabled":{"type":"boolean", "value":false},
                    "token":{"kind":"secret_omitted"}
                }))
            ),
            expected_argument("field", expected_text("enabled")),
            expected_argument("default", expected_data(json!(true))),
            expected_argument(
                "path",
                expected_data(
                    json!({"encoding":"utf8", "value":"/synthetic/observation-imports.toml"})
                )
            ),
            expected_argument("index", expected_data(json!(4)))
        ]),
        expected_text("false"),
    );
}

fn write_xlsx(path: &Path) {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let entries = [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Data</t></is></c><c r="B1" t="inlineStr"><is><t>Peso</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>2026-01-02</t></is></c><c r="B2"><v>70.5</v></c></row></sheetData></worksheet>"#,
        ),
    ];
    for (name, contents) in entries {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(contents.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}
