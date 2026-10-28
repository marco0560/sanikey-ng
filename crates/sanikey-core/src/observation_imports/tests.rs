use super::{ensure_observation_imports_current, import_observations};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

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
