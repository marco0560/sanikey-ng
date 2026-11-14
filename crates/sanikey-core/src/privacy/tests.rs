//! Test sintetici delle invarianti dei percorsi runtime.

use super::{normalize, privacy_error, resolve_non_strict, validate_runtime_privacy};
use crate::config::{AccountsConfig, parse_accounts_data};
use crate::logging::{Component, Level, Session};
use crate::metadata::prepare_people_metadata;
use crate::operation::ErrorCode;
use serde_json::{Value as JsonValue, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use toml::Value;

struct TestDirectory(PathBuf);

/// Gestisce una root temporanea per le verifiche di privacy.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-privacy-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Pulisce la root sintetica alla fine del test.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config_with_paths(root: &Path, paths: [&Path; 3]) -> AccountsConfig {
    let person = toml::map::Map::from_iter([
        ("id".to_owned(), Value::String("patient-a".to_owned())),
        (
            "display_name".to_owned(),
            Value::String("Patient A".to_owned()),
        ),
        (
            "source_documents".to_owned(),
            Value::String(paths[0].display().to_string()),
        ),
        (
            "metadata_directory".to_owned(),
            Value::String(paths[1].display().to_string()),
        ),
        (
            "local_build".to_owned(),
            Value::String(paths[2].display().to_string()),
        ),
        ("usb_uuid".to_owned(), Value::String("1A2B-3C4D".to_owned())),
    ]);
    let root_table = toml::map::Map::from_iter([
        (
            "global".to_owned(),
            Value::Table(toml::map::Map::from_iter([(
                "config_version".to_owned(),
                Value::Integer(1),
            )])),
        ),
        (
            "person".to_owned(),
            Value::Array(vec![Value::Table(person)]),
        ),
    ]);
    parse_accounts_data(&Value::Table(root_table), root.join("config/accounts.toml"))
        .expect("configurazione sintetica valida")
}

fn expected_path_trace(path: &Path) -> JsonValue {
    json!({
        "kind": "data",
        "value": {"encoding": "utf8", "value": path.to_str().expect("path sintetico UTF-8")}
    })
}

fn expected_secret_path_trace() -> JsonValue {
    json!({"kind": "secret_omitted"})
}

fn expected_arguments(name: &str, content: JsonValue) -> JsonValue {
    json!([{"name": name, "content": content}])
}

fn assert_complete_trace_calls(
    records: &[JsonValue],
    function: &str,
    expected: Vec<(JsonValue, JsonValue)>,
) {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), expected.len(), "numero ingressi: {function}");
    let mut unmatched = entries;
    for (arguments, result) in expected {
        let index = unmatched
            .iter()
            .position(|entry| entry["arguments"] == arguments)
            .unwrap_or_else(|| panic!("argomenti completi assenti per {function}: {arguments}"));
        let entry = unmatched.remove(index);
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        let exits: Vec<_> = records
            .iter()
            .filter(|record| {
                record["type"] == "function_exit"
                    && record["function"] == function
                    && record["call_id"] == call_id
            })
            .collect();
        assert_eq!(exits.len(), 1, "uscita correlata: {function}#{call_id}");
        assert_eq!(exits[0]["outcome"], "returned", "esito: {function}");
        assert!(
            exits[0]["duration_ms"].is_u64(),
            "durata assente: {function}#{call_id}"
        );
        assert_eq!(exits[0]["result"], result, "risultato: {function}");
    }
    assert!(unmatched.is_empty());
}

#[test]
fn traced_path_helpers_match_complete_inputs_results_and_durations() {
    let directory = TestDirectory::new();
    let root = directory.path();
    let resolve_input = root.join("missing/child");
    let resolve_result = resolve_input.clone();
    let normalize_input = root.join("alpha/../beta/./gamma");
    let normalize_result = root.join("beta/gamma");
    let secret_path = root.join("generated/api_key=SYNTHETIC-PRIVACY-PATH-SECRET");
    let message = "synthetic privacy failure";
    let (session, log_path) =
        Session::create_in_directory_with_level(&root.join("logs"), Component::Core, Level::Trace)
            .expect("sessione trace sintetica");

    let (resolved, normalized, normalized_secret, error) = session.scoped(|| {
        (
            resolve_non_strict(&resolve_input).expect("percorso inesistente risolto"),
            normalize(&normalize_input),
            normalize(&secret_path),
            privacy_error(message),
        )
    });
    drop(session);
    assert_eq!(resolved, resolve_result);
    assert_eq!(normalized, normalize_result);
    assert_eq!(normalized_secret, secret_path);
    assert_eq!(error.code(), ErrorCode::Privacy);
    assert_eq!(error.message(), message);

    let log = fs::read_to_string(log_path).expect("log trace sintetico");
    assert!(!log.contains("SYNTHETIC-PRIVACY-PATH-SECRET"));
    assert!(log.contains("secret_omitted"));
    let records: Vec<JsonValue> = log
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
        .collect();
    let resolve_arguments = expected_arguments("path", expected_path_trace(&resolve_input));
    let resolve_result = json!({
        "kind": "data",
        "value": {
            "outcome": "succeeded",
            "path": expected_path_trace(&resolve_result),
        }
    });
    assert_complete_trace_calls(
        &records,
        "privacy::resolve_non_strict",
        vec![(resolve_arguments, resolve_result)],
    );

    let normalize_resolve_arguments =
        expected_arguments("path", expected_path_trace(&resolve_input));
    let normalize_direct_arguments =
        expected_arguments("path", expected_path_trace(&normalize_input));
    let normalize_resolve_result = json!({
        "kind": "data",
        "value": {"path": expected_path_trace(&resolve_input)},
    });
    let normalize_direct_result = json!({
        "kind": "data",
        "value": {"path": expected_path_trace(&normalize_result)},
    });
    assert_complete_trace_calls(
        &records,
        "privacy::normalize",
        vec![
            (
                normalize_resolve_arguments.clone(),
                normalize_resolve_result.clone(),
            ),
            (normalize_resolve_arguments, normalize_resolve_result),
            (normalize_direct_arguments, normalize_direct_result),
            (
                expected_arguments("path", expected_secret_path_trace()),
                json!({
                    "kind": "data",
                    "value": {"path": expected_secret_path_trace()},
                }),
            ),
        ],
    );

    assert_complete_trace_calls(
        &records,
        "privacy::privacy_error",
        vec![(
            expected_arguments("message", json!({"kind": "data", "value": message})),
            json!({
                "kind": "data",
                "value": {"code": "privacy", "message": message},
            }),
        )],
    );
}

#[test]
fn runtime_privacy_rejects_versioned_repository_paths() {
    let directory = TestDirectory::new();
    let root = directory.path();
    let config = config_with_paths(
        root,
        [
            Path::new("docs/source"),
            Path::new("local-data/patient-a/metadata"),
            Path::new("local-data/generated/patient-a"),
        ],
    );
    let error = validate_runtime_privacy(&config, root).expect_err("path docs vietato");

    assert_eq!(error.code(), ErrorCode::Privacy);
    assert!(
        error
            .message()
            .contains("contenuto versionato del repository")
    );
}

#[test]
fn runtime_privacy_allows_paths_under_local_data_directories() {
    let directory = TestDirectory::new();
    let root = directory.path();
    let config = config_with_paths(
        root,
        [
            Path::new("local-data/patient-a/documents"),
            Path::new("config/patient-a/metadata"),
            Path::new("generated/patient-a"),
        ],
    );

    validate_runtime_privacy(&config, root).expect("percorsi locali ammessi senza Git");
}

#[test]
fn runtime_privacy_allows_data_outside_the_repository() {
    let directory = TestDirectory::new();
    let root = directory.path().join("repository");
    fs::create_dir_all(&root).expect("root sintetica");
    let outside = directory.path().join("private-data");
    let config = config_with_paths(
        &root,
        [
            outside.join("source").as_path(),
            outside.join("metadata").as_path(),
            outside.join("generated").as_path(),
        ],
    );

    validate_runtime_privacy(&config, &root).expect("percorsi esterni ammessi");
}

#[test]
fn metadata_preflight_fails_before_returning_prepared_people() {
    let directory = TestDirectory::new();
    let root = directory.path();
    let metadata = root.join("local-data/patient-a/metadata");
    fs::create_dir_all(&metadata).expect("directory metadata sintetica");
    fs::write(metadata.join("problems.toml"), "[[problem]\nid='broken'\n")
        .expect("metadati invalidi sintetici");
    let config = config_with_paths(
        root,
        [
            Path::new("local-data/patient-a/documents"),
            Path::new("local-data/patient-a/metadata"),
            Path::new("local-data/generated/patient-a"),
        ],
    );

    let error = prepare_people_metadata(&config, root).expect_err("preflight deve fallire");

    assert_eq!(error.code(), ErrorCode::Configuration);
    assert!(error.message().contains("TOML non valido"));
}

#[test]
fn metadata_preflight_preserves_original_documents_without_git() {
    let directory = TestDirectory::new();
    let root = directory.path();
    let source = root.join("local-data/patient-a/documents");
    fs::create_dir_all(&source).expect("directory originali sintetici");
    let original = source.join("report.pdf");
    let source_bytes = b"synthetic immutable source";
    fs::write(&original, source_bytes).expect("originale sintetico");
    let config = config_with_paths(
        root,
        [
            Path::new("local-data/patient-a/documents"),
            Path::new("local-data/patient-a/metadata"),
            Path::new("local-data/generated/patient-a"),
        ],
    );

    let prepared = prepare_people_metadata(&config, root)
        .expect("preflight indipendente da Git su directory non-repository");

    assert_eq!(prepared.len(), 1);
    assert_eq!(
        fs::read(original).expect("originale dopo preflight"),
        source_bytes
    );
}

#[test]
fn runtime_privacy_trace_records_representable_inputs_and_result_only_at_trace_level() {
    for (level, expect_trace) in [(Level::Events, false), (Level::Trace, true)] {
        let directory = TestDirectory::new();
        let root = directory.path().join("repository");
        fs::create_dir_all(&root).expect("root sintetica");
        let private_path = "docs/private-synthetic-path-marker";
        let config = config_with_paths(
            &root,
            [
                Path::new(private_path),
                Path::new("local-data/patient-a/metadata"),
                Path::new("generated/api_key=synthetic-privacy-config-secret"),
            ],
        );
        let (session, path) = Session::create_in_directory_with_level(
            &directory.path().join("logs"),
            Component::Core,
            level,
        )
        .expect("sessione sintetica");

        let result = session.scoped(|| validate_runtime_privacy(&config, &root));
        let error = result.expect_err("il percorso docs sintetico va rifiutato");
        assert_eq!(error.code(), ErrorCode::Privacy);
        let log = fs::read_to_string(path).expect("log sintetico leggibile");

        if expect_trace {
            assert!(log.contains("privacy::validate_runtime_privacy"));
            assert!(log.contains("privacy::resolve_non_strict"));
            assert!(log.contains("privacy::privacy_error"));
            assert!(log.contains("\"name\":\"message\""));
            assert!(log.contains("\"code\":\"privacy\""));
            assert!(log.contains("private-synthetic-path-marker"));
            assert!(log.contains("secret_omitted"));
            assert!(!log.contains("synthetic-privacy-config-secret"));
            assert!(log.contains("rust_debug"));
            assert!(log.contains("Patient A"));
            assert!(log.contains("contenuto versionato del repository"));
        } else {
            assert!(!log.contains("privacy::validate_runtime_privacy"));
            assert!(!log.contains("private-synthetic-path-marker"));
            assert!(!log.contains("contenuto versionato del repository"));
        }
    }
}

#[cfg(unix)]
#[test]
fn runtime_privacy_trace_encodes_non_utf8_paths_as_unix_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = TestDirectory::new();
    let non_utf8_path = PathBuf::from(OsString::from_vec(vec![b'p', 0xff]));
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");

    let normalized = session.scoped(|| super::normalize(&non_utf8_path));

    assert_eq!(normalized, non_utf8_path);
    let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
    assert!(log.contains("privacy::normalize"));
    assert!(log.contains("unix_bytes"));
    assert!(log.contains("255"));
    assert!(!log.contains("unsupported_type"));
}
