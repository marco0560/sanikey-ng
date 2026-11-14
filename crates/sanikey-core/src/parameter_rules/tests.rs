//! Fixture sintetiche per caricamento e applicazione delle regole.

use super::{
    ParameterBuildResult, RuleDecision, build_parameter_slices, evaluate_parameter_candidates,
    load_parameter_rules, load_patient_parameter_rules,
};
use crate::config::SearchDictionary;
use crate::logging::{Component, Level, Session};
use crate::operation::ErrorCode;
use crate::parameter_slices::ParameterCandidate;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

/// Gestisce i file temporanei di configurazione delle regole.
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

/// Rimuove i file di configurazione temporanei.
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

fn traced<R>(directory: &TestDirectory, operation: impl FnOnce() -> R) -> (R, String, Vec<Value>) {
    static NEXT_TRACE: AtomicUsize = AtomicUsize::new(0);
    let logs = directory.0.join(format!(
        "logs-{}",
        NEXT_TRACE.fetch_add(1, Ordering::Relaxed)
    ));
    let (session, path) =
        Session::create_in_directory_with_level(&logs, Component::Core, Level::Trace)
            .expect("sessione Trace sintetica");
    let result = session.scoped(operation);
    session.finish(Component::Core).expect("chiusura Trace");
    let contents = fs::read_to_string(path).expect("log Trace sintetico");
    let records = contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
        .collect();
    (result, contents, records)
}

fn function_span<'a>(records: &'a [Value], function: &str) -> (&'a Value, &'a Value) {
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

/// Builds the JSON shape required by the trace contract from an explicit value.
fn expected_debug_trace(value: &str) -> Value {
    serde_json::json!({
        "kind":"data",
        "value":{"format":"rust_debug", "value":value}
    })
}

/// Builds the JSON shape required by the trace contract from explicit text.
fn expected_text_trace(value: &str) -> Value {
    serde_json::json!({
        "kind":"data",
        "value":{
            "format":"text_redacted_known_secret_assignments",
            "value":value
        }
    })
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

#[test]
fn builds_exact_converted_points_and_rejects_invalid_units_and_ranges() {
    let directory = TestDirectory::new();
    let path = directory.0.join("parameters.toml");
    fs::write(&path, rule_toml("Emoglobina", 1)).expect("rule file");
    let rules = load_parameter_rules(&path, &dictionary()).expect("valid rules");
    let mut converted = candidate("hb", "Hb: 137 g/l");
    converted.stable_id = "converted".into();
    converted.raw_value = "137".into();
    converted.parsed_value = Some(137.0);
    converted.number_format = Some("integer".into());
    converted.raw_unit = Some("g/l".into());
    converted.document_href = Some("../documenti/referto.txt".into());
    let mut missing_unit = converted.clone();
    missing_unit.stable_id = "missing-unit".into();
    missing_unit.raw_unit = None;
    let mut unknown_unit = converted.clone();
    unknown_unit.stable_id = "unknown-unit".into();
    unknown_unit.raw_unit = Some("mg/dL".into());
    let mut out_of_range = converted.clone();
    out_of_range.stable_id = "out-of-range".into();
    out_of_range.raw_value = "999".into();
    out_of_range.parsed_value = Some(999.0);

    let result = build_parameter_slices(
        &[converted, missing_unit, unknown_unit, out_of_range],
        &rules,
    );

    assert_eq!(result.series.len(), 1);
    assert_eq!(result.series[0].id, "emoglobina");
    assert_eq!(result.series[0].unit.as_deref(), Some("g/dL"));
    assert_eq!(result.points.len(), 1);
    assert_eq!(result.points[0].numeric_value, Some(13.7));
    assert_eq!(result.points[0].parsed_value, Some(137.0));
    assert_eq!(result.points[0].raw_unit.as_deref(), Some("g/l"));
    assert_eq!(result.points[0].normalized_unit.as_deref(), Some("g/dL"));
    assert!(result.series[0].warn_duplicate_same_day);
    assert_eq!(
        result.points[0].document_href.as_deref(),
        Some("../documenti/referto.txt")
    );
    assert_eq!(
        result.points[0].source_reference,
        "../documenti/referto.txt"
    );
    assert_eq!(result.points[0].rule_id.as_deref(), Some("emoglobina"));
    let reasons = result
        .decisions
        .iter()
        .map(|decision| {
            (
                decision.candidate_id.as_str(),
                decision.reason_code.as_str(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(reasons["converted"], "ACCEPTED_CONFIGURED_SYNONYM");
    assert_eq!(reasons["missing-unit"], "REJECTED_MISSING_UNIT");
    assert_eq!(reasons["unknown-unit"], "REJECTED_UNKNOWN_UNIT");
    assert_eq!(reasons["out-of-range"], "REJECTED_OUT_OF_RANGE");

    let empty = build_parameter_slices(&[], &rules);
    assert_eq!(empty, ParameterBuildResult::default());
}

#[test]
fn normalizes_configured_unit_typography_and_applies_missing_unit_policies() {
    let directory = TestDirectory::new();
    let path = directory.0.join("parameters.toml");
    fs::write(&path, rule_toml("Emoglobina", 1)).expect("rule file");
    let loaded = load_parameter_rules(&path, &dictionary()).expect("valid rules");
    let mut rule = loaded.rules[0].clone();
    rule.units = vec!["g/dl".into()];
    rule.canonical_unit = Some("g/dl".into());
    rule.conversions.clear();
    let mut candidate = candidate("hb", "Hb: 13,7 g/DL");
    candidate.raw_unit = Some("g/DL".into());
    let configured = build_parameter_slices(
        &[candidate.clone()],
        &super::ParameterRules {
            discovery: loaded.discovery.clone(),
            rules: vec![rule.clone()],
        },
    );
    assert_eq!(
        configured.points[0].normalized_unit.as_deref(),
        Some("g/dl")
    );
    assert_eq!(configured.series[0].unit_variant, None);

    candidate.raw_unit = None;
    rule.unit_policy = "allowed-but-unknown".into();
    rule.units.clear();
    rule.canonical_unit = None;
    rule.assumed_unit = None;
    let optional = build_parameter_slices(
        &[candidate.clone()],
        &super::ParameterRules {
            discovery: loaded.discovery.clone(),
            rules: vec![rule.clone()],
        },
    );
    assert_eq!(optional.points[0].normalized_unit, None);
    assert_eq!(optional.points[0].series_id, "emoglobina");

    rule.unit_policy = "assume-configured-unit".into();
    rule.assumed_unit = Some("g/dL".into());
    let assumed = build_parameter_slices(
        &[candidate],
        &super::ParameterRules {
            discovery: loaded.discovery,
            rules: vec![rule],
        },
    );
    assert_eq!(assumed.points[0].normalized_unit.as_deref(), Some("g/dL"));
    assert_eq!(assumed.series[0].id, "emoglobina--g-dl");
}

#[test]
fn trace_records_parameter_pipeline_and_redacts_secret_assignments() {
    for level in [Level::Events, Level::Trace] {
        let directory = TestDirectory::new();
        let rules_path = directory.0.join("parameters.toml");
        fs::write(&rules_path, rule_toml("Emoglobina", 1)).expect("rule file");
        let log_directory = directory.0.join("logs");
        let (session, log_path) =
            Session::create_in_directory_with_level(&log_directory, Component::Core, level)
                .expect("synthetic log session");

        session.scoped(|| {
            let rules = load_parameter_rules(&rules_path, &dictionary()).expect("valid rules");
            let mut input = candidate(
                "hb",
                "Hb: 13,7 g/dL\npassword = synthetic-parameter-rules-secret",
            );
            input.context = input.original_line.clone();
            let decisions = evaluate_parameter_candidates(&[input.clone()], &rules.rules);
            assert!(decisions[0].accepted);
            let result = build_parameter_slices(&[input], &rules);
            assert_eq!(result.points.len(), 1);
        });
        session.finish(Component::Core).expect("finish log session");
        drop(session);

        let contents = fs::read_to_string(&log_path).expect("read synthetic log");
        if level == Level::Trace {
            assert!(contents.contains("parameter_rules::load_parameter_rules"));
            assert!(contents.contains("parameter_rules::evaluate_parameter_candidates"));
            assert!(contents.contains("parameter_rules::build_parameter_slices"));
            assert!(contents.contains("Hb: 13,7 g/dL"));
            assert!(contents.contains("secret_omitted"));
            assert!(!contents.contains("synthetic-parameter-rules-secret"));
        } else {
            assert!(!contents.contains("parameter_rules::"));
            assert!(!contents.contains("Hb: 13,7 g/dL"));
        }
    }
}

#[test]
fn trace_redacts_secret_keys_recursively_in_parsed_parameter_toml() {
    let directory = TestDirectory::new();
    let path = directory.0.join("parameters.toml");
    fs::write(
        &path,
        "[unexpected]\ndescription = \"synthetic-config-note\"\n[unexpected.nested]\napi_key = \"synthetic-config-secret\"\n",
    )
    .expect("malformed synthetic rules");
    let log_directory = directory.0.join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("synthetic log session");
    session.scoped(|| {
        let error = load_parameter_rules(&path, &dictionary()).expect_err("unknown root field");
        assert_eq!(error.code(), ErrorCode::Configuration);
    });
    session.finish(Component::Core).expect("finish log session");
    drop(session);

    let contents = fs::read_to_string(log_path).expect("read synthetic log");
    assert!(contents.contains("synthetic-config-note"));
    assert!(contents.contains("secret_omitted"));
    assert!(!contents.contains("synthetic-config-secret"));
}

#[test]
fn synthetic_trace_records_complete_parameter_rule_spans() {
    let directory = TestDirectory::new();

    let normalized_input = "Ｈｂ";
    let (normalized, normalized_log, normalized_records) =
        traced(&directory, || super::normalized_nfkc(normalized_input));
    assert_eq!(normalized, "hb");
    let (enter, exit) = function_span(&normalized_records, "parameter_rules::normalized_nfkc");
    assert_eq!(
        enter["arguments"],
        serde_json::json!([{
            "name":"value",
            "content":expected_text_trace("Ｈｂ")
        }])
    );
    assert_eq!(exit["result"], expected_debug_trace("\"hb\""));
    assert!(normalized_log.contains("Ｈｂ"));

    let secret_number = "password=synthetic-decimal-secret";
    let (parsed, decimal_log, decimal_records) = traced(&directory, || {
        super::parse_decimal(secret_number, "integer")
    });
    assert_eq!(parsed, None);
    let (enter, exit) = function_span(&decimal_records, "parameter_rules::parse_decimal");
    assert_eq!(
        enter["arguments"],
        serde_json::json!([
            {"name":"raw", "content":expected_text_trace("password=[secret_omitted]")},
            {"name":"format", "content":expected_text_trace("integer")}
        ])
    );
    assert_eq!(exit["result"], expected_debug_trace("None"));
    assert!(!decimal_log.contains("synthetic-decimal-secret"));

    let unit_input = " g／dL\u{00a0} ";
    let (unit, unit_log, unit_records) = traced(&directory, || super::normalize_unit(unit_input));
    assert_eq!(unit, "g/dL");
    let (enter, exit) = function_span(&unit_records, "parameter_rules::normalize_unit");
    assert_eq!(
        enter["arguments"],
        serde_json::json!([{
            "name":"value",
            "content":expected_text_trace(" g／dL\u{00a0} ")
        }])
    );
    assert_eq!(exit["result"], expected_debug_trace("\"g/dL\""));
    assert!(unit_log.contains(unit_input.trim()));

    let rules_path = directory.0.join("parameters.toml");
    fs::write(&rules_path, rule_toml("Emoglobina", 1)).expect("regole sintetiche");
    let rules = load_parameter_rules(&rules_path, &dictionary()).expect("regole valide");
    let rule = &rules.rules[0];

    let range_value = Decimal::new(999, 0);
    let range_unit = Some("g/dL".to_owned());
    let expected_range: Result<(Decimal, Option<String>), &str> = Err("REJECTED_OUT_OF_RANGE");
    let (range, _range_log, range_records) = traced(&directory, || {
        super::check_range(range_value, range_unit.clone(), rule)
    });
    assert_eq!(range, expected_range);
    let (enter, exit) = function_span(&range_records, "parameter_rules::check_range");
    assert_eq!(
        enter["arguments"],
        serde_json::json!([
            {"name":"value", "content":expected_debug_trace("999")},
            {"name":"unit", "content":expected_debug_trace(&format!("{range_unit:#?}"))},
            {"name":"rule", "content":expected_debug_trace(&format!("{rule:#?}"))}
        ])
    );
    assert_eq!(
        exit["result"],
        expected_debug_trace(&format!("{expected_range:#?}"))
    );

    let mut conversion_candidate = candidate("hb", "Hb: 137 g/l");
    conversion_candidate.raw_value = "137".into();
    conversion_candidate.parsed_value = Some(137.0);
    conversion_candidate.number_format = Some("integer".into());
    conversion_candidate.raw_unit = Some("g/l".into());
    let expected_conversion = Ok((Decimal::new(137, 1), Some("g/dL".to_owned())));
    let (conversion, _conversion_log, conversion_records) = traced(&directory, || {
        super::value_and_unit(&conversion_candidate, rule)
    });
    assert_eq!(conversion, expected_conversion);
    let (enter, exit) = function_span(&conversion_records, "parameter_rules::value_and_unit");
    assert_eq!(
        enter["arguments"],
        serde_json::json!([
            {
                "name":"candidate",
                "content":expected_debug_trace(&format!("{conversion_candidate:#?}"))
            },
            {"name":"rule", "content":expected_debug_trace(&format!("{rule:#?}"))}
        ])
    );
    assert_eq!(
        exit["result"],
        expected_debug_trace(&format!("{expected_conversion:#?}"))
    );

    let mut private_candidate =
        candidate("hb", "Hb: 13,7 g/dL\npassword = synthetic-candidate-secret");
    private_candidate.context = private_candidate.original_line.clone();
    let candidates = vec![private_candidate];
    let decisions = vec![RuleDecision {
        candidate_id: "candidate-hb".into(),
        rule_id: "emoglobina".into(),
        accepted: true,
        reason_code: "ACCEPTED_CONFIGURED_SYNONYM".into(),
    }];
    let (evaluated, evaluation_log, evaluation_records) = traced(&directory, || {
        super::evaluate_parameter_candidates(&candidates, &rules.rules)
    });
    assert_eq!(evaluated, decisions);
    assert!(!evaluation_log.contains("synthetic-candidate-secret"));
    let (enter, exit) = function_span(
        &evaluation_records,
        "parameter_rules::evaluate_parameter_candidates",
    );
    assert_eq!(
        enter["arguments"],
        serde_json::json!([
            {
                "name":"candidates",
                "content":expected_debug_trace(
                    &format!("{candidates:#?}").replace(
                        "synthetic-candidate-secret",
                        "[secret_omitted]"
                    )
                )
            },
            {"name":"rules", "content":expected_debug_trace(&format!("{:#?}", rules.rules))}
        ])
    );
    assert_eq!(
        exit["result"],
        expected_debug_trace(&format!("{decisions:#?}"))
    );

    let (built, build_log, build_records) = traced(&directory, || {
        super::build_parameter_slices(&candidates, &rules)
    });
    assert_eq!(built.points.len(), 1);
    assert_eq!(built.decisions, decisions);
    assert!(!build_log.contains("synthetic-candidate-secret"));
    let (enter, exit) = function_span(&build_records, "parameter_rules::build_parameter_slices");
    assert_eq!(
        enter["arguments"],
        serde_json::json!([
            {
                "name":"candidates",
                "content":expected_debug_trace(
                    &format!("{candidates:#?}").replace(
                        "synthetic-candidate-secret",
                        "[secret_omitted]"
                    )
                )
            },
            {"name":"rules", "content":expected_debug_trace(&format!("{rules:#?}"))}
        ])
    );
    assert_eq!(exit["result"]["value"]["format"], "rust_debug");
    let build_output = exit["result"]["value"]["value"]
        .as_str()
        .expect("risultato Debug del build");
    for expected_field in ["series:", "points:", "decisions:", "candidate-hb"] {
        assert!(
            build_output.contains(expected_field),
            "campo {expected_field}"
        );
    }

    let message = "Configurazione sintetica api_key=synthetic-error-secret";
    let (error, error_log, error_records) = traced(&directory, || super::config_error(message));
    assert_eq!(error.code(), ErrorCode::Configuration);
    assert_eq!(error.message(), message);
    assert!(!error_log.contains("synthetic-error-secret"));
    let (enter, exit) = function_span(&error_records, "parameter_rules::config_error");
    assert_eq!(
        enter["arguments"],
        serde_json::json!([{
            "name":"message",
            "content":expected_text_trace(
                "Configurazione sintetica api_key=[secret_omitted]"
            )
        }])
    );
    assert_eq!(
        exit["result"],
        expected_debug_trace(
            "OperationError {\n    code: Configuration,\n    message: \"Configurazione sintetica api_key=[secret_omitted]\",\n}"
        )
    );
}
