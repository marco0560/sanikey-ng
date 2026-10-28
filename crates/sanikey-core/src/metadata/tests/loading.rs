//! Casi di caricamento e isolamento delle directory metadata.

use super::common::{TestDirectory, metadata};
use crate::metadata::load_curated_metadata;

#[test]
fn load_curated_metadata_reads_supported_files() {
    let directory = TestDirectory::new();
    directory.write(
        "problems.toml",
        r#"[[problem]]
id = "hypertension"
title = "Hypertension"
status = "active"
"#,
    );
    directory.write(
        "medications.toml",
        r#"[[medication]]
id = "drug-a"
name = "Drug A"
active_ingredient = "Ingredient A"
form = "compresse"
strength_per_unit = "100 mg"

[[medication]]
id = "supplement-a"
name = "Supplement A"
"#,
    );
    directory.write(
        "therapies.toml",
        r#"[[therapy]]
medication_id = "drug-a"
start_date = "2026-01-01"
dosage = "1 tablet"
schedule = ["risveglio", "cena"]
instructions = "dopo il pasto"
"#,
    );
    directory.write(
        "medication_leaflets.toml",
        r#"[[leaflet]]
medication_id = "drug-a"
codice_sis = "123"
aic6 = "456"
source_fingerprint = "a"

[[unavailable]]
medication_id = "supplement-a"
reason = "non_aifa"
"#,
    );
    directory.write(
        "procedures.toml",
        r#"[[procedure]]
id = "procedure-a"
title = "Procedure A"
date = "2026-01-02"
status = "completed"
"#,
    );
    directory.write(
        "observations.toml",
        r#"[[observation]]
id = "observation-a"
kind = "weight"
value = "70 kg"
date = "2026-01-03"
"#,
    );
    directory.write(
        "observations/series.toml",
        r#"[[series]]
id = "weight"
name = "Peso"
value_type = "numeric"
unit = "kg"
synonyms = ["peso", "massa"]
"#,
    );
    directory.write(
        "observations/points.toml",
        r#"[[point]]
id = "point-a"
series_id = "weight"
observation_date = "2026-01-03"
source_type = "manual"
source_reference = "diario"
numeric_value = 70.5
"#,
    );
    directory.write(
        "timeline_events.toml",
        r#"[[event]]
id = "event-a"
title = "Event A"
start_date = "2026-01-04"
links = ["procedure-a"]
"#,
    );
    directory.write(
        "document_tags.toml",
        r#"[tags]
"20260101 Report.pdf" = ["report", "test"]
"#,
    );
    directory.write(
        "clinical_summary.toml",
        "summary = \"\"\"Synthetic summary.\\nSecond line.\"\"\"\n",
    );

    let loaded = metadata(directory.path());

    assert_eq!(loaded.problems[0].id, "hypertension");
    assert_eq!(loaded.medications[0].name, "Drug A");
    assert_eq!(loaded.medications[0].form.as_deref(), Some("compresse"));
    assert_eq!(loaded.medication_leaflets[0].aic6, "456");
    assert_eq!(
        loaded.medication_leaflet_exclusions[0].medication_id,
        "supplement-a"
    );
    assert_eq!(loaded.therapies[0].id, "therapy-drug-a-1");
    assert_eq!(loaded.therapies[0].schedule, ["risveglio", "cena"]);
    assert_eq!(loaded.procedures[0].status, "completed");
    assert_eq!(loaded.observations[0].value, "70 kg");
    assert_eq!(loaded.observation_series[0].synonyms, ["peso", "massa"]);
    assert_eq!(loaded.observation_points[0].numeric_value, Some(70.5));
    assert_eq!(loaded.timeline_events[0].links, ["procedure-a"]);
    assert_eq!(
        loaded.document_tags["20260101 Report.pdf"],
        ["report", "test"]
    );
    assert_eq!(
        loaded.clinical_summary.as_deref(),
        Some("Synthetic summary.\nSecond line.")
    );
}

#[test]
fn load_curated_metadata_allows_missing_directory() {
    let directory = TestDirectory::new();
    let loaded = load_curated_metadata(directory.path().join("missing"))
        .expect("directory metadata assente è facoltativa");
    assert!(loaded.problems.is_empty());
    assert_eq!(loaded.clinical_summary, None);
}

#[test]
fn load_curated_metadata_keeps_people_isolated() {
    let directory = TestDirectory::new();
    let first = directory.path().join("patient-a/metadata");
    let second = directory.path().join("patient-b/metadata");
    std::fs::create_dir_all(&first).expect("directory paziente A");
    std::fs::create_dir_all(&second).expect("directory paziente B");
    std::fs::write(
        first.join("problems.toml"),
        "[[problem]]\nid='a'\ntitle='Patient A only'\n",
    )
    .expect("metadati paziente A");
    std::fs::write(
        second.join("problems.toml"),
        "[[problem]]\nid='b'\ntitle='Patient B only'\n",
    )
    .expect("metadati paziente B");
    let metadata_a = load_curated_metadata(first).expect("metadati A validi");
    let metadata_b = load_curated_metadata(second).expect("metadati B validi");
    assert_eq!(metadata_a.problems[0].id, "a");
    assert_eq!(metadata_b.problems[0].id, "b");
}
