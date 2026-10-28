use super::{
    ParameterDiagnosticExtra, write_parameter_extraction_reports, write_parameter_rejections,
    write_parameter_reports,
};
use crate::metadata::{DocumentRecord, ObservationPoint, ObservationSeries};
use crate::parameter_rules::{ParameterBuildResult, RuleDecision};
use crate::parameter_slices::{DiscoverySettings, discover_candidates};
use crate::pdf::ExtractedText;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sanikey-parameter-reports-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn document(id: &str, date: &str) -> DocumentRecord {
    DocumentRecord {
        document_id: id.into(),
        patient_id: "patient-a".into(),
        path: PathBuf::from(format!("/{id}.txt")),
        title: "Referto sintetico".into(),
        category: "Laboratorio".into(),
        kind: "text".into(),
        sha256: "a".repeat(64),
        date: Some(date.into()),
        series: None,
        tags: Vec::new(),
        origin: "source".into(),
        container_id: None,
        internal_path: None,
    }
}

#[test]
fn writes_candidate_report_and_disabled_toml_scaffold() {
    let directory = TestDirectory::new();
    let discovery = discover_candidates(
        &[
            document("doc-a", "2026-01-02"),
            document("doc-b", "2026-02-03"),
        ],
        &[
            ExtractedText {
                document_id: "doc-a".into(),
                text: "Hb: 13.7 g/dL\n".into(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            },
            ExtractedText {
                document_id: "doc-b".into(),
                text: "Hb: <14.1 g/dL\n".into(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            },
        ],
        None,
        &DiscoverySettings::default(),
        None,
    )
    .expect("discovery");

    let paths = write_parameter_reports(&directory.0, &discovery, None).expect("reports");

    assert_eq!(
        paths.candidates,
        directory.0.join("reports/parameter-candidates.json")
    );
    assert_eq!(
        paths.proposed_rules,
        directory.0.join("reports/parameter-rules.proposed.toml")
    );
    assert_eq!(paths.slices_json, None);
    assert_eq!(paths.slices_javascript, None);
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&paths.candidates).expect("candidate report"))
            .expect("JSON report");
    assert_eq!(report["schema_version"], "1.0");
    assert_eq!(report["analyzed_lines"], 2);
    assert_eq!(
        report["candidates"].as_array().expect("candidates").len(),
        2
    );
    assert_eq!(report["candidates"][0]["raw_value"], "13.7");

    let proposal = fs::read_to_string(directory.0.join("reports/parameter-rules.proposed.toml"))
        .expect("proposal");
    let parsed: toml::Value = toml::from_str(&proposal).expect("valid proposal TOML");
    assert!(parsed["generated_at"].as_str().is_some());
    let rule = &parsed["parameters"]["hb"];
    assert_eq!(rule["enabled"].as_bool(), Some(false));
    assert_eq!(rule["value_type"].as_str(), Some("qualified-scalar"));
    assert_eq!(rule["number_formats"][0].as_str(), Some("decimal-point"));
    assert_eq!(rule["units"][0].as_str(), Some("g/dL"));
}

#[test]
fn writes_machine_and_javascript_slice_exports_with_stable_bytes() {
    let directory = TestDirectory::new();
    let result = ParameterBuildResult {
        series: vec![ObservationSeries {
            id: "emoglobina".into(),
            name: "Emoglobina".into(),
            value_type: "scalar".into(),
            unit: Some("g/dL".into()),
            description: None,
            warn_duplicate_same_day: true,
            synonyms: vec!["hb".into()],
            parameter_rule_id: Some("emoglobina".into()),
            parameter_rule_version: Some(1),
            parameter_rule_digest: Some("a".repeat(64)),
            unit_variant: None,
        }],
        points: vec![ObservationPoint {
            id: "point-1".into(),
            series_id: "emoglobina".into(),
            observation_date: "2026-01-02".into(),
            source_type: "document-extraction".into(),
            source_reference: "referto.txt".into(),
            numeric_value: Some(13.7),
            text_value: None,
            systolic: None,
            diastolic: None,
            pulse: None,
            note: None,
            source_kind: "document-extraction".into(),
            document_id: Some("doc-a".into()),
            document_href: Some("../documents/referto.txt".into()),
            document_title: Some("Referto sintetico".into()),
            document_category: Some("Laboratorio".into()),
            source_text_digest: Some("b".repeat(64)),
            original_line: Some("Hb: 13.7 g/dL".into()),
            line_number: Some(1),
            page_number: None,
            character_start: Some(0),
            character_end: Some(13),
            matched_label: Some("hb".into()),
            raw_value: Some("13.7".into()),
            parsed_value: Some(13.7),
            raw_unit: Some("g/dL".into()),
            normalized_unit: Some("g/dL".into()),
            qualifier: None,
            rule_id: Some("emoglobina".into()),
            rule_version: Some(1),
            rule_digest: Some("a".repeat(64)),
            reason_code: Some("ACCEPTED_CONFIGURED_SYNONYM".into()),
        }],
        decisions: vec![RuleDecision {
            candidate_id: "candidate-1".into(),
            rule_id: "emoglobina".into(),
            accepted: true,
            reason_code: "ACCEPTED_CONFIGURED_SYNONYM".into(),
        }],
    };

    let (json_path, script_path) =
        write_parameter_extraction_reports(&directory.0, &result).expect("exports");
    let first = fs::read(&json_path).expect("JSON bytes");
    write_parameter_extraction_reports(&directory.0, &result).expect("repeat exports");
    assert_eq!(first, fs::read(&json_path).expect("repeat JSON bytes"));
    let payload: serde_json::Value = serde_json::from_slice(&first).expect("slice JSON");
    assert_eq!(payload["schema_version"], "1.0");
    assert_eq!(payload["series"][0]["parameter_rule_id"], "emoglobina");
    assert_eq!(
        payload["points"][0]["document_href"],
        "../documents/referto.txt"
    );
    assert_eq!(payload["decisions"][0]["accepted"], true);
    let script = fs::read_to_string(script_path).expect("JavaScript export");
    assert!(script.starts_with("window.SANIKEY_PARAMETER_SLICES = {"));
    assert!(script.ends_with(";\n"));
}

#[test]
fn writes_accepted_rejected_json_and_aligned_text_with_extra_diagnostics() {
    let directory = TestDirectory::new();
    let discovery = discover_candidates(
        &[
            document("doc-a", "2026-01-02"),
            document("doc-b", "2026-02-03"),
        ],
        &[
            ExtractedText {
                document_id: "doc-a".into(),
                text: "Hb: 13.7 g/dL\n".into(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            },
            ExtractedText {
                document_id: "doc-b".into(),
                text: "Hb: <14.1 g/dL\n".into(),
                warnings: Vec::new(),
                page_spans: Vec::new(),
            },
        ],
        None,
        &DiscoverySettings::default(),
        None,
    )
    .expect("discovery");
    let result = ParameterBuildResult {
        series: Vec::new(),
        points: Vec::new(),
        decisions: vec![
            RuleDecision {
                candidate_id: discovery.candidates[0].stable_id.clone(),
                rule_id: "emoglobina".into(),
                accepted: true,
                reason_code: "ACCEPTED_CONFIGURED_SYNONYM".into(),
            },
            RuleDecision {
                candidate_id: discovery.candidates[1].stable_id.clone(),
                rule_id: "emoglobina".into(),
                accepted: false,
                reason_code: "REJECTED_SPECIMEN_AMBIGUOUS".into(),
            },
        ],
    };
    let extra = [ParameterDiagnosticExtra {
        candidate_id: String::new(),
        parameter: "peso".into(),
        document: "altro-referto.txt".into(),
        line: 3,
        value: "Peso: 5 kg".into(),
        reason_code: "REJECTED_OUT_OF_RANGE".into(),
    }];

    let rejected_paths = write_parameter_rejections(&directory.0, &discovery, &result, &extra)
        .expect("decision reports");

    assert_eq!(
        rejected_paths.0,
        directory.0.join("reports/parameter-rejected.json")
    );
    assert_eq!(
        rejected_paths.1,
        directory.0.join("reports/parameter-rejected.txt")
    );
    let accepted: serde_json::Value = serde_json::from_slice(
        &fs::read(directory.0.join("reports/parameter-accepted.json")).expect("accepted JSON"),
    )
    .expect("accepted report");
    let rejected: serde_json::Value =
        serde_json::from_slice(&fs::read(&rejected_paths.0).expect("rejected JSON"))
            .expect("rejected report");
    assert_eq!(
        accepted["accepted"]
            .as_array()
            .expect("accepted rows")
            .len(),
        1
    );
    assert_eq!(
        rejected["rejected"]
            .as_array()
            .expect("rejected rows")
            .len(),
        2
    );
    assert_eq!(rejected["rejected"][1]["parameter"], "peso");
    let accepted_text = fs::read_to_string(directory.0.join("reports/parameter-accepted.txt"))
        .expect("accepted text");
    let rejected_text = fs::read_to_string(&rejected_paths.1).expect("rejected text");
    assert!(accepted_text
        .lines()
        .next()
        .is_some_and(|header| header.starts_with("Parametro") && header.contains("| Documento")));
    assert!(accepted_text.contains("ACCEPTED_CONFIGURED_SYNONYM"));
    assert!(rejected_text.contains("REJECTED_SPECIMEN_AMBIGUOUS"));
    assert!(rejected_text.contains("REJECTED_OUT_OF_RANGE"));
}
