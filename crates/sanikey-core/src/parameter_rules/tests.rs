use super::{
    RuleDecision, evaluate_parameter_candidates, load_parameter_rules, load_patient_parameter_rules,
};
use crate::config::SearchDictionary;
use crate::operation::ErrorCode;
use crate::parameter_slices::ParameterCandidate;
use indexmap::IndexMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sanikey-parameter-rules-{}-{id}",
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

fn dictionary() -> SearchDictionary {
    SearchDictionary {
        terms: IndexMap::from([(
            "emoglobina".into(),
            vec!["Hb".into(), "HGB".into(), "emoglobina".into()],
        )]),
        months: IndexMap::new(),
    }
}

fn candidate(label: &str, line: &str) -> ParameterCandidate {
    ParameterCandidate {
        stable_id: format!("candidate-{label}"),
        document_id: "document-1".into(),
        document_date: Some("2026-01-02".into()),
        document_href: None,
        document_title: "Referto sintetico".into(),
        document_name: "report.txt".into(),
        document_category: "Laboratorio".into(),
        source_text_digest: "a".repeat(64),
        line_number: 1,
        character_start: 0,
        character_end: line.chars().count(),
        original_line: line.into(),
        normalized_label: label.into(),
        raw_value: "13,7".into(),
        parsed_value: Some(13.7),
        number_format: Some("decimal-comma".into()),
        qualifier: None,
        raw_unit: Some("g/dL".into()),
        prefix_context: String::new(),
        suffix_context: String::new(),
        reason_code: None,
        page_number: None,
        context: line.into(),
        page_label: "non applicabile".into(),
        section_specimen: None,
    }
}

fn rule_toml(display_name: &str, version: i64) -> String {
    format!(
        r#"
        [parameters.emoglobina]
        display_name = "{display_name}"
        term = "emoglobina"
        version = {version}
        value_type = "qualified-scalar"
        number_formats = ["integer", "decimal-point", "decimal-comma"]
        unit_policy = "required"
        enabled = true
        units = ["g/dL", "g/l"]
        canonical_unit = "g/dL"
        minimum = 1
        maximum = 30

        [[parameters.emoglobina.conversions]]
        from_unit = "g/l"
        to_unit = "g/dL"
        multiplier = 0.1
        offset = 0
        version = 1
        "#
    )
}

#[test]
fn resolves_synonyms_conversions_and_stable_rule_digest() {
    let directory = TestDirectory::new();
    let path = directory.0.join("parameters.toml");
    fs::write(&path, rule_toml("Emoglobina", 1)).expect("rule file");

    let first = load_parameter_rules(&path, &dictionary()).expect("valid rules");
    let second = load_parameter_rules(&path, &dictionary()).expect("repeat load");
    let rule = &first.rules[0];

    assert_eq!(rule.id, "emoglobina");
    assert_eq!(rule.synonyms, ["emoglobina", "hb", "hgb"]);
    assert_eq!(
        rule.number_formats,
        ["decimal-comma", "decimal-point", "integer"]
    );
    assert_eq!(rule.units, ["g/dL", "g/l"]);
    assert_eq!(rule.canonical_unit.as_deref(), Some("g/dL"));
    assert_eq!(rule.minimum.expect("minimum").to_string(), "1");
    assert_eq!(rule.maximum.expect("maximum").to_string(), "30");
    assert_eq!(rule.conversions.len(), 1);
    assert_eq!(rule.conversions[0].multiplier.to_string(), "0.1");
    assert_eq!(rule.conversions[0].offset.to_string(), "0");
    assert_eq!(
        rule.digest,
        "853a7bf9d62194da2160729940688641fcc07336bf81b22463a9c5e02afef49a"
    );
    assert_eq!(rule.digest, second.rules[0].digest);
}

#[test]
fn patient_rule_replaces_the_common_rule_and_inherits_discovery_by_default() {
    let directory = TestDirectory::new();
    let common = directory.0.join("common.toml");
    let patient = directory.0.join("patient.toml");
    fs::write(
        &common,
        format!(
            "[discovery]\nmin_occurrences = 3\n{}",
            rule_toml("Comune", 1)
        ),
    )
    .expect("common rules");
    fs::write(&patient, rule_toml("Paziente", 2)).expect("patient rules");

    let merged =
        load_patient_parameter_rules(&common, &patient, &dictionary()).expect("merged rules");

    assert_eq!(merged.rules.len(), 1);
    assert_eq!(merged.rules[0].display_name, "Paziente");
    assert_eq!(merged.rules[0].version, 2);
    assert_eq!(merged.discovery.min_occurrences, 3);
}

#[test]
fn patient_discovery_overrides_common_and_unknown_fields_are_rejected() {
    let directory = TestDirectory::new();
    let common = directory.0.join("common.toml");
    let patient = directory.0.join("patient.toml");
    fs::write(&common, rule_toml("Comune", 1)).expect("common rules");
    fs::write(
        &patient,
        format!(
            "[discovery]\nmin_occurrences = 4\n{}",
            rule_toml("Paziente", 2)
        ),
    )
    .expect("patient rules");
    let merged = load_patient_parameter_rules(&common, &patient, &dictionary())
        .expect("explicit patient discovery");
    assert_eq!(merged.discovery.min_occurrences, 4);

    let malformed = directory.0.join("malformed.toml");
    fs::write(&malformed, "unexpected = true\n").expect("malformed rules");
    let error = load_parameter_rules(&malformed, &dictionary()).expect_err("unknown field");
    assert_eq!(error.code(), ErrorCode::Configuration);
    assert!(error.message().contains("sconosciuti"));
}

#[test]
fn evaluates_exact_synonyms_and_emits_first_rejection_reason() {
    let directory = TestDirectory::new();
    let path = directory.0.join("parameters.toml");
    fs::write(&path, rule_toml("Emoglobina", 1)).expect("rule file");
    let rules = load_parameter_rules(&path, &dictionary()).expect("valid rules");
    let accepted = candidate("hb", "Hb: 13,7 g/dL");
    let rejected = candidate("emoglobina totale", "Emoglobina totale: 13,7 g/dL");

    let decisions = evaluate_parameter_candidates(&[rejected, accepted], &rules.rules);

    assert_eq!(
        decisions,
        [
            RuleDecision {
                candidate_id: "candidate-emoglobina totale".into(),
                rule_id: "emoglobina".into(),
                accepted: false,
                reason_code: "REJECTED_LABEL".into(),
            },
            RuleDecision {
                candidate_id: "candidate-hb".into(),
                rule_id: "emoglobina".into(),
                accepted: true,
                reason_code: "ACCEPTED_CONFIGURED_SYNONYM".into(),
            },
        ]
    );
}

#[test]
fn rejects_ambiguous_rule_matches_and_ignores_disabled_rules() {
    let directory = TestDirectory::new();
    let path = directory.0.join("parameters.toml");
    fs::write(&path, rule_toml("Emoglobina", 1)).expect("rule file");
    let mut rules = load_parameter_rules(&path, &dictionary())
        .expect("valid rules")
        .rules;
    let mut duplicate = rules[0].clone();
    duplicate.id = "emoglobina-seconda-regola".into();
    duplicate.digest = "b".repeat(64);
    let mut disabled = duplicate.clone();
    disabled.id = "emoglobina-disabilitata".into();
    disabled.enabled = false;
    rules.extend([duplicate, disabled]);

    let decisions = evaluate_parameter_candidates(&[candidate("hb", "Hb: 13,7 g/dL")], &rules);

    assert_eq!(decisions.len(), 2);
    assert!(decisions.iter().all(|decision| {
        !decision.accepted && decision.reason_code == "REJECTED_MULTIPLE_RULE_MATCHES"
    }));
}

#[test]
fn evaluates_specimen_category_context_and_qualifier_constraints() {
    let directory = TestDirectory::new();
    let path = directory.0.join("parameters.toml");
    let source = rule_toml("Emoglobina", 1)
        .replace("value_type = \"qualified-scalar\"", "value_type = \"scalar\"")
        .replace(
            "canonical_unit = \"g/dL\"",
            "canonical_unit = \"g/dL\"\ndocument_categories = [\"Laboratorio\"]\nspecimen = \"serum\"\ndefault_specimen_labels = [\"hb\"]\nrequired_context = [\"dosaggio\"]\nexcluded_context = [\"controllo\"]",
        );
    fs::write(&path, source).expect("rule file");
    let rules = load_parameter_rules(&path, &dictionary()).expect("valid rules");
    let mut valid = candidate("hb", "Dosaggio Hb: 13,7 g/dL");
    valid.section_specimen = Some("serum".into());
    let mut wrong_specimen = valid.clone();
    wrong_specimen.stable_id = "wrong-specimen".into();
    wrong_specimen.section_specimen = Some("urine".into());
    let mut default_specimen = valid.clone();
    default_specimen.stable_id = "default-specimen".into();
    default_specimen.section_specimen = None;
    let mut ambiguous_specimen = valid.clone();
    ambiguous_specimen.stable_id = "ambiguous-specimen".into();
    ambiguous_specimen.normalized_label = "hgb".into();
    ambiguous_specimen.section_specimen = None;
    let mut missing_context = valid.clone();
    missing_context.stable_id = "missing-context".into();
    missing_context.original_line = "Hb: 13,7 g/dL".into();
    let mut excluded_context = valid.clone();
    excluded_context.stable_id = "excluded-context".into();
    excluded_context.original_line = "Dosaggio Hb: 13,7 g/dL controllo".into();
    let mut unqualified = valid.clone();
    unqualified.stable_id = "unqualified".into();
    unqualified.qualifier = Some("<".into());
    let mut wrong_category = valid.clone();
    wrong_category.stable_id = "wrong-category".into();
    wrong_category.document_category = "Imaging".into();
    let mut missing_date = valid.clone();
    missing_date.stable_id = "missing-date".into();
    missing_date.document_date = None;
    let mut invalid_number = valid.clone();
    invalid_number.stable_id = "invalid-number".into();
    invalid_number.parsed_value = None;
    invalid_number.reason_code = Some("REJECTED_INVALID_NUMBER_FORMAT".into());

    let decisions = evaluate_parameter_candidates(
        &[
            valid,
            wrong_specimen,
            default_specimen,
            ambiguous_specimen,
            missing_context,
            excluded_context,
            unqualified,
            wrong_category,
            missing_date,
            invalid_number,
        ],
        &rules.rules,
    );
    let reasons = decisions
        .into_iter()
        .map(|decision| (decision.candidate_id, decision.reason_code))
        .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(reasons["candidate-hb"], "ACCEPTED_CONFIGURED_SYNONYM");
    assert_eq!(reasons["wrong-specimen"], "REJECTED_SPECIMEN_MISMATCH");
    assert_eq!(reasons["default-specimen"], "ACCEPTED_DEFAULT_SPECIMEN");
    assert_eq!(reasons["ambiguous-specimen"], "REJECTED_SPECIMEN_AMBIGUOUS");
    assert_eq!(
        reasons["missing-context"],
        "REJECTED_REQUIRED_CONTEXT_MISSING"
    );
    assert_eq!(reasons["excluded-context"], "REJECTED_EXCLUDED_CONTEXT");
    assert_eq!(reasons["unqualified"], "REJECTED_QUALIFIER_UNSUPPORTED");
    assert_eq!(reasons["wrong-category"], "REJECTED_DOCUMENT_CATEGORY");
    assert_eq!(reasons["missing-date"], "REJECTED_DOCUMENT_DATE_MISSING");
    assert_eq!(reasons["invalid-number"], "REJECTED_INVALID_NUMBER_FORMAT");
}
