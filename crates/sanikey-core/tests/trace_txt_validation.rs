//! Verifica sintetica delle span del validatore TXT su manifest non supportato.

use sanikey_core::logging::{Component, Level, Session};
use sanikey_core::operation::ErrorCode;
use sanikey_core::simulated_export::validate_txt_simulation;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Gestisce i file sintetici usati dai test di tracciamento TXT.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-txt-validation-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory sintetica");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Rimuove i file temporanei del test al termine del ciclo di vita della fixture.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn records(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("log Trace sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect()
}

fn trace_path(path: &Path) -> Value {
    json!({
        "kind": "data",
        "value": {
            "kind": "data",
            "value": {"encoding": "utf8", "value": path.to_str().expect("path UTF-8")}
        }
    })
}

fn argument(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn assert_span(
    records: &[Value],
    function: &str,
    expected_arguments: Value,
    expected_result: Value,
) -> u64 {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), 1, "una sola entrata per {function}");
    let entry = entries[0];
    assert_eq!(entry["component"], "core");
    assert_eq!(
        entry["arguments"], expected_arguments,
        "argomenti {function}"
    );
    let call_id = entry["call_id"].as_u64().expect("call_id numerico");
    let exits: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_exit"
                && record["function"] == function
                && record["call_id"] == call_id
        })
        .collect();
    assert_eq!(exits.len(), 1, "una sola uscita per {function}#{call_id}");
    assert_eq!(exits[0]["outcome"], "returned");
    assert!(exits[0]["duration_ms"].as_u64().is_some());
    assert_eq!(exits[0]["result"], expected_result, "risultato {function}");
    call_id
}

#[test]
fn txt_validator_traces_complete_error_inputs_results_and_omitted_manifest_bytes() {
    let directory = TestDirectory::new();
    let target = directory.path().join("image");
    fs::create_dir_all(&target).expect("directory immagine sintetica");
    let manifest_path = target.join("SANIKEY-MANIFEST.json");
    fs::write(&manifest_path, br#"{"schema_version":0}"#).expect("manifest sintetico");
    let manifest_bytes = fs::read(&manifest_path).expect("lettura fixture sintetica");
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let error = session
        .scoped(|| validate_txt_simulation(&target))
        .expect_err("schema sintetico non supportato");
    assert_eq!(error.code(), ErrorCode::Configuration);
    assert_eq!(error.message(), "versione manifest non supportata");
    session.finish(Component::Core).expect("chiusura sessione");

    let trace = records(&log_path);
    let expected_error = json!({
        "kind": "data",
        "value": [
            argument("code", json!({"kind": "data", "value": "configuration"})),
            argument(
                "message",
                json!({
                    "kind": "data",
                    "value": {
                        "format": "text_redacted_known_secret_assignments",
                        "value": "versione manifest non supportata"
                    }
                })
            )
        ]
    });
    let target_arguments = json!([argument("target", trace_path(&target))]);
    let outer_id = assert_span(
        &trace,
        "simulated_export::validate_txt_simulation",
        target_arguments.clone(),
        expected_error.clone(),
    );
    let inner_id = assert_span(
        &trace,
        "simulated_export::validate_txt_simulation_inner",
        target_arguments,
        expected_error.clone(),
    );
    let parse_id = assert_span(
        &trace,
        "simulated_export::parse_manifest_json",
        json!([
            argument("path", trace_path(&manifest_path)),
            argument(
                "byte_length",
                json!({"kind": "data", "value": manifest_bytes.len()})
            ),
            argument(
                "raw_bytes",
                json!({
                    "kind": "data",
                    "value": [
                        argument("state", json!({"kind": "data", "value": "omitted"})),
                        argument(
                            "reason",
                            json!({"kind": "data", "value": "json_is_recorded_after_parsing"})
                        )
                    ]
                })
            )
        ]),
        json!({"kind": "data", "value": {"schema_version": 0}}),
    );
    let config_error_id = assert_span(
        &trace,
        "simulated_export::configuration_error",
        json!([argument(
            "message",
            json!({
                "kind": "data",
                "value": {
                    "format": "text_redacted_known_secret_assignments",
                    "value": "versione manifest non supportata"
                }
            })
        )]),
        expected_error,
    );
    let mut call_ids = vec![outer_id, inner_id, parse_id, config_error_id];
    call_ids.sort_unstable();
    call_ids.dedup();
    assert_eq!(call_ids.len(), 4, "call_id distinti per gli span attesi");
}
