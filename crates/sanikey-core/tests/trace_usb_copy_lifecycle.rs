//! Verifica Trace sintetica della copia directory e dei confini dei byte trasferiti.

use sanikey_core::logging::{Component, Level, Session};
use sanikey_core::operation::{CancellationToken, OperationEvent, OperationOutcome};
use sanikey_core::usb_copy::{UsbCopyResult, mirror_directory};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Crea e fornisce accesso alla directory sintetica usata dal test.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-usb-copy-lifecycle-{}-{}",
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

/// Rimuove la directory sintetica quando il fixture termina.
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
        "value": {"encoding": "utf8", "value": path.to_str().expect("path UTF-8")}
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
fn mirror_directory_traces_exact_copy_results_and_external_byte_omission() {
    let directory = TestDirectory::new();
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir_all(&source).expect("directory sorgente sintetica");
    let source_file = source.join("payload.bin");
    let target_file = target.join("payload.bin");
    let contents = b"synthetic copy payload";
    fs::write(&source_file, contents).expect("contenuto sintetico");
    let expected_digest = format!("{:x}", Sha256::digest(contents));
    let expected_run = UsbCopyResult {
        root: target.clone(),
        copied_files: 1,
        reused_files: 0,
        removed_files: 0,
        bytes_copied: contents.len() as u64,
    };
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let result =
        session.scoped(|| mirror_directory(&source, &target, &CancellationToken::default()));
    assert!(matches!(
        &result.outcome,
        OperationOutcome::Succeeded(copy) if copy == &expected_run
    ));
    assert_eq!(
        result.events,
        vec![
            OperationEvent::Started {
                operation: "usb.copy"
            },
            OperationEvent::Progress {
                phase: sanikey_core::operation::OperationPhase::Processing,
                completed: 1,
                total: Some(1),
            },
            OperationEvent::Succeeded,
        ]
    );
    assert_eq!(fs::read(&target_file).expect("file copiato"), contents);
    session.finish(Component::Core).expect("chiusura sessione");

    let trace = records(&log_path);
    let operation_result = json!({
        "kind": "data",
        "value": {
            "output": {
                "kind": "data",
                "value": {
                    "outcome": {
                        "state": "succeeded",
                        "root": trace_path(&target),
                        "copied_files": 1,
                        "reused_files": 0,
                        "removed_files": 0,
                        "bytes_copied": contents.len(),
                    },
                    "events": [
                        {"state": "started", "operation": "usb.copy"},
                        {"state": "progress", "phase": "Processing", "completed": 1, "total": 1},
                        {"state": "succeeded"}
                    ],
                    "successful_paths": {
                        "root": trace_path(&target),
                        "copied_files": 1,
                        "reused_files": 0,
                        "removed_files": 0,
                        "bytes_copied": contents.len(),
                    }
                }
            }
        }
    });
    let outer_id = assert_span(
        &trace,
        "usb_copy::mirror_directory",
        json!([
            argument("source", trace_path(&source)),
            argument("target", trace_path(&target)),
            argument(
                "cancellation_requested",
                json!({"kind": "data", "value": false})
            )
        ]),
        operation_result,
    );

    let inner_result = json!({
        "kind": "data",
        "value": {
            "output": {
                "kind": "data",
                "value": {
                    "root": trace_path(&target),
                    "copied_files": 1,
                    "reused_files": 0,
                    "removed_files": 0,
                    "bytes_copied": contents.len(),
                }
            }
        }
    });
    let inner_id = assert_span(
        &trace,
        "usb_copy::mirror_directory_inner",
        json!([
            argument("source", trace_path(&source)),
            argument("target", trace_path(&target)),
            argument(
                "cancellation_requested",
                json!({"kind": "data", "value": false})
            )
        ]),
        inner_result,
    );

    let copy_arguments = json!([
        argument("source", trace_path(&source_file)),
        argument("target", trace_path(&target_file)),
        argument(
            "expected_sha256",
            json!({"kind": "data", "value": expected_digest})
        ),
        argument(
            "transferred_contents",
            json!({"kind": "omitted", "reason": "external_component"})
        )
    ]);
    let copy_entry = trace
        .iter()
        .find(|record| {
            record["type"] == "function_enter"
                && record["function"] == "usb_copy::copy_file_atomically"
        })
        .expect("ingresso copy_file_atomically");
    assert_eq!(copy_entry["arguments"], copy_arguments);
    let observed_copy_id = copy_entry["call_id"].as_u64().expect("call_id numerico");
    let observed_copy_exit = trace
        .iter()
        .find(|record| {
            record["type"] == "function_exit"
                && record["function"] == "usb_copy::copy_file_atomically"
                && record["call_id"] == observed_copy_id
        })
        .expect("uscita copy_file_atomically");
    let temporary = observed_copy_exit["result"]["value"]["output"]["value"]["temporary_path"]["value"]["value"]
        .as_str()
        .expect("path temporaneo tracciato");
    let filename = Path::new(temporary)
        .file_name()
        .expect("nome file temporaneo")
        .to_str()
        .expect("nome temporaneo UTF-8");
    let expected_prefix = format!(".payload.bin.sanikey-{}-", std::process::id());
    assert!(filename.starts_with(&expected_prefix));
    assert!(filename[expected_prefix.len()..].parse::<usize>().is_ok());
    assert_eq!(Path::new(temporary).parent(), target_file.parent());
    assert_eq!(observed_copy_exit["outcome"], "returned");
    assert!(observed_copy_exit["duration_ms"].as_u64().is_some());
    let copy_id = assert_span(
        &trace,
        "usb_copy::copy_file_atomically",
        copy_arguments,
        json!({
            "kind": "data",
            "value": {
                "output": {
                    "kind": "data",
                    "value": {
                        "bytes_copied": contents.len(),
                        "temporary_path": trace_path(Path::new(temporary)),
                        "verified_sha256": expected_digest,
                        "result": {
                            "kind": "data",
                            "value": {
                                "format": "text_redacted_known_secret_assignments",
                                "value": format!("{:#?}", contents.len() as u64)
                            }
                        }
                    }
                }
            }
        }),
    );
    let mut call_ids = vec![outer_id, inner_id, copy_id];
    call_ids.sort_unstable();
    call_ids.dedup();
    assert_eq!(
        call_ids.len(),
        3,
        "call_id distinti per gli span verificati"
    );
    assert_eq!(observed_copy_id, copy_id);
}
