//! Copertura sintetica del lifecycle Trace e della proiezione degli eventi operativi.

use sanikey_core::logging::{Component, Level, Session, TraceValue};
use sanikey_core::operation::{
    CancellationToken, OperationEvent, OperationOutcome, OperationPhase, run_operation,
    run_operation_with_trace_result,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Gestisce la directory temporanea sintetica del test.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-operation-lifecycle-{}-{}",
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

/// Rimuove la directory sintetica quando termina il test.
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

fn argument(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn assert_spans(records: &[Value], function: &str, expected: &[(Value, Value)]) -> Vec<u64> {
    let mut entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), expected.len(), "numero ingressi {function}");
    let mut call_ids = Vec::with_capacity(expected.len());
    for (expected_arguments, expected_result) in expected {
        let index = entries
            .iter()
            .position(|entry| entry["arguments"] == *expected_arguments)
            .unwrap_or_else(|| panic!("argomenti attesi assenti per {function}"));
        let entry = entries.remove(index);
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        let exits: Vec<_> = records
            .iter()
            .filter(|record| {
                record["type"] == "function_exit"
                    && record["function"] == function
                    && record["call_id"] == call_id
            })
            .collect();
        assert_eq!(exits.len(), 1, "uscita correlata {function}#{call_id}");
        assert_eq!(exits[0]["outcome"], "returned");
        assert!(exits[0]["duration_ms"].as_u64().is_some());
        assert_eq!(exits[0]["result"], *expected_result, "risultato {function}");
        call_ids.push(call_id);
    }
    assert!(entries.is_empty());
    call_ids
}

#[test]
fn operation_lifecycle_traces_complete_calls_progress_and_omission_contract() {
    let directory = TestDirectory::new();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let legacy = session.scoped(|| {
        run_operation("synthetic.legacy", &CancellationToken::default(), |_| {
            Ok::<_, sanikey_core::operation::OperationError>("legacy output")
        })
    });
    assert!(matches!(
        &legacy.outcome,
        OperationOutcome::Succeeded(output) if *output == "legacy output"
    ));
    assert_eq!(
        legacy.events,
        vec![
            OperationEvent::Started {
                operation: "synthetic.legacy"
            },
            OperationEvent::Succeeded,
        ]
    );

    let projected = session.scoped(|| {
        run_operation_with_trace_result(
            "synthetic.progress",
            &CancellationToken::default(),
            |context| {
                context.set_phase(OperationPhase::Processing);
                context.report_progress(2, Some(3));
                Ok::<_, sanikey_core::operation::OperationError>(17_u64)
            },
            |value| TraceValue::Data(json!({"kind": "synthetic_count", "value": value})),
        )
    });
    assert!(matches!(projected.outcome, OperationOutcome::Succeeded(17)));
    assert_eq!(
        projected.events,
        vec![
            OperationEvent::Started {
                operation: "synthetic.progress"
            },
            OperationEvent::Progress {
                phase: OperationPhase::Processing,
                completed: 2,
                total: Some(3),
            },
            OperationEvent::Succeeded,
        ]
    );
    session.finish(Component::Core).expect("chiusura sessione");

    let trace = records(&log_path);
    let action_omitted = json!({"kind": "omitted", "reason": "unsupported_type"});
    let legacy_result = json!({
        "kind": "data",
        "value": {
            "outcome": "succeeded",
            "value": {"kind": "omitted", "reason": "unsupported_type"},
            "events": [
                {"kind": "started", "operation": "synthetic.legacy"},
                {"kind": "succeeded"}
            ]
        }
    });
    let projected_result = json!({
        "kind": "data",
        "value": {
            "outcome": "succeeded",
            "value": {"kind": "data", "value": {"kind": "synthetic_count", "value": 17}},
            "events": [
                {"kind": "started", "operation": "synthetic.progress"},
                {"kind": "progress", "phase": "processing", "completed": 2, "total": 3},
                {"kind": "succeeded"}
            ]
        }
    });
    let operation_ids = assert_spans(
        &trace,
        "operation::run_operation",
        &[
            (
                json!([
                    argument(
                        "operation",
                        json!({"kind": "data", "value": "synthetic.legacy"})
                    ),
                    argument(
                        "cancellation_requested",
                        json!({"kind": "data", "value": false})
                    ),
                    argument("action", action_omitted.clone())
                ]),
                legacy_result,
            ),
            (
                json!([
                    argument(
                        "operation",
                        json!({"kind": "data", "value": "synthetic.progress"})
                    ),
                    argument(
                        "cancellation_requested",
                        json!({"kind": "data", "value": false})
                    ),
                    argument("action", action_omitted)
                ]),
                projected_result,
            ),
        ],
    );
    let phase_ids = assert_spans(
        &trace,
        "operation::logging_phase",
        &[
            (
                json!([argument(
                    "phase",
                    json!({"kind": "data", "value": "Processing"})
                )]),
                json!({"kind": "data", "value": "Processing"}),
            ),
            (
                json!([argument(
                    "phase",
                    json!({"kind": "data", "value": "Processing"})
                )]),
                json!({"kind": "data", "value": "Processing"}),
            ),
        ],
    );
    let mut ids = operation_ids;
    ids.extend(phase_ids);
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids.len(),
        4,
        "call_id distinti per tutte le span verificate"
    );
}
