use super::{load_parameter_rules, load_patient_parameter_rules};
use crate::config::SearchDictionary;
use crate::operation::ErrorCode;
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
