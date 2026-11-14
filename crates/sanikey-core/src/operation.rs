//! Typed operation outcomes, progress events, and cooperative cancellation.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use crate::logging::{
    Activity, Component, FailureCategory, Operation, Outcome, ProgressPhase, Session, TraceField,
    TraceOmission, TraceValue, trace_current, trace_sensitive_text,
};

/// Stable machine-readable categories for application failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    /// Invalid or unavailable configuration.
    Configuration,
    /// A repository privacy rule was violated.
    Privacy,
    /// A removable-volume operation failed validation.
    Usb,
}

impl ErrorCode {
    /// Returns the stable identifier used by machine-facing output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Privacy => "privacy",
            Self::Usb => "usb",
        }
    }
}

/// An application failure with a machine code and an Italian user message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationError {
    code: ErrorCode,
    message: String,
}

/// Espone le operazioni per costruire e interrogare errori applicativi.
impl OperationError {
    /// Creates an error without coupling its machine category to its wording.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let message = message.into();
        let trace = trace_current(Component::Core, "operation::OperationError::new", || {
            vec![
                TraceField::new("code", TraceValue::Data(json!(code.as_str()))),
                TraceField::new("message", trace_sensitive_text(&message)),
            ]
        });
        let error = Self { code, message };
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(
                json!({"code": error.code.as_str(), "message": trace_sensitive_text(&error.message).to_json()}),
            ));
        }
        error
    }

    /// Returns the machine-readable category.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// Returns the Italian message intended for people.
    #[must_use]
    pub fn message(&self) -> &str {
        let trace = trace_current(
            Component::Core,
            "operation::OperationError::message",
            || {
                vec![TraceField::new(
                    "self",
                    TraceValue::Data(
                        json!({"code": self.code.as_str(), "message": trace_sensitive_text(&self.message).to_json()}),
                    ),
                )]
            },
        );
        if let Some(trace) = trace {
            let _ = trace.finish(trace_sensitive_text(&self.message));
        }
        &self.message
    }
}

/// Formatta l'errore con il messaggio destinato alle persone.
impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let trace = trace_current(Component::Core, "operation::OperationError::fmt", || {
            vec![
                TraceField::new(
                    "self",
                    TraceValue::Data(
                        json!({"code": self.code.as_str(), "message": trace_sensitive_text(&self.message).to_json()}),
                    ),
                ),
                TraceField::new(
                    "formatter",
                    TraceValue::Omitted(TraceOmission::UnsupportedType),
                ),
            ]
        });
        let result = formatter.write_str(self.message());
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!({"ok": result.is_ok()})));
        }
        result
    }
}

/// Integra l'errore applicativo con il protocollo standard di Rust.
impl std::error::Error for OperationError {}

/// A coarse phase reported by a running operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationPhase {
    /// Checking preconditions before work begins.
    Preparing,
    /// Processing the operation's inputs.
    Processing,
    /// Writing or verifying final outputs.
    Finalizing,
}

/// A typed event emitted during an operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationEvent {
    /// The operation has started; the identifier is stable and machine-facing.
    Started { operation: &'static str },
    /// The operation reports completed work within its current phase.
    Progress {
        phase: OperationPhase,
        completed: u64,
        total: Option<u64>,
    },
    /// The operation returned successfully.
    Succeeded,
    /// The operation failed with a categorized error.
    Failed(OperationError),
    /// The operation stopped at a cooperative cancellation point.
    Cancelled,
}

fn trace_operation_events(events: &[OperationEvent]) -> Value {
    Value::Array(
        events
            .iter()
            .map(|event| match event {
                OperationEvent::Started { operation } => {
                    json!({"kind": "started", "operation": operation})
                }
                OperationEvent::Progress {
                    phase,
                    completed,
                    total,
                } => json!({
                    "kind": "progress",
                    "phase": match phase {
                        OperationPhase::Preparing => "preparing",
                        OperationPhase::Processing => "processing",
                        OperationPhase::Finalizing => "finalizing",
                    },
                    "completed": completed,
                    "total": total,
                }),
                OperationEvent::Succeeded => json!({"kind": "succeeded"}),
                OperationEvent::Failed(error) => json!({
                    "kind": "failed",
                    "code": error.code().as_str(),
                    "message": trace_sensitive_text(error.message()).to_json(),
                }),
                OperationEvent::Cancelled => json!({"kind": "cancelled"}),
            })
            .collect(),
    )
}

/// The terminal result of an operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationOutcome<T> {
    /// The operation completed and returned its result.
    Succeeded(T),
    /// The operation failed.
    Failed(OperationError),
    /// The operation stopped after cancellation was requested.
    Cancelled,
}

/// A cancellation flag that can be shared with an operation controller.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

/// Richiede l'annullamento e ne espone lo stato condiviso.
impl CancellationToken {
    /// Requests cancellation; the operation observes it at its next safe point.
    pub fn cancel(&self) {
        let trace = trace_current(
            Component::Core,
            "operation::CancellationToken::cancel",
            || {
                vec![TraceField::new(
                    "previously_cancelled",
                    TraceValue::Data(json!(self.0.load(Ordering::Relaxed))),
                )]
            },
        );
        self.0.store(true, Ordering::Relaxed);
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!(null)));
        }
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        let trace = trace_current(
            Component::Core,
            "operation::CancellationToken::is_cancelled",
            || {
                vec![TraceField::new(
                    "self_cancelled_at_entry",
                    TraceValue::Data(json!(self.0.load(Ordering::Relaxed))),
                )]
            },
        );
        let cancelled = self.0.load(Ordering::Relaxed);
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!(cancelled)));
        }
        cancelled
    }
}

/// The result and ordered event history of one operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationRun<T> {
    /// Terminal result.
    pub outcome: OperationOutcome<T>,
    /// Events emitted from start through the single terminal event.
    pub events: Vec<OperationEvent>,
}

/// Context available to an operation while it reports progress.
pub struct OperationContext<'a> {
    cancellation: &'a CancellationToken,
    phase: OperationPhase,
    events: Vec<OperationEvent>,
    logging_operation: Option<&'a Operation>,
}

/// Gestisce fase, progresso e controlli di annullamento dell'operazione.
impl OperationContext<'_> {
    /// Changes the phase used by subsequent progress events.
    pub fn set_phase(&mut self, phase: OperationPhase) {
        let trace = trace_current(
            Component::Core,
            "operation::OperationContext::set_phase",
            || {
                vec![
                    TraceField::new(
                        "previous_phase",
                        TraceValue::Data(json!(format!("{:?}", self.phase))),
                    ),
                    TraceField::new(
                        "previous_event_count",
                        TraceValue::Data(json!(self.events.len())),
                    ),
                    TraceField::new(
                        "cancellation_requested",
                        TraceValue::Data(json!(self.cancellation.0.load(Ordering::Relaxed))),
                    ),
                    TraceField::new("phase", TraceValue::Data(json!(format!("{phase:?}")))),
                ]
            },
        );
        self.phase = phase;
        if let Some(operation) = self.logging_operation {
            let _ = operation.phase(logging_phase(phase));
        }
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!(null)));
        }
    }

    /// Reports completed work without exposing presentation-specific text.
    pub fn report_progress(&mut self, completed: u64, total: Option<u64>) {
        let trace = trace_current(
            Component::Core,
            "operation::OperationContext::report_progress",
            || {
                vec![
                    TraceField::new(
                        "phase",
                        TraceValue::Data(json!(format!("{:?}", self.phase))),
                    ),
                    TraceField::new(
                        "previous_event_count",
                        TraceValue::Data(json!(self.events.len())),
                    ),
                    TraceField::new(
                        "cancellation_requested",
                        TraceValue::Data(json!(self.cancellation.0.load(Ordering::Relaxed))),
                    ),
                    TraceField::new("completed", TraceValue::Data(json!(completed))),
                    TraceField::new("total", TraceValue::Data(json!(total))),
                ]
            },
        );
        self.events.push(OperationEvent::Progress {
            phase: self.phase,
            completed,
            total,
        });
        if let Some(operation) = self.logging_operation {
            let _ = operation.progress(logging_phase(self.phase), completed, total);
        }
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!(null)));
        }
    }

    /// Checks cancellation at an operation-defined safe point.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        let trace = trace_current(
            Component::Core,
            "operation::OperationContext::is_cancelled",
            || {
                vec![
                    TraceField::new(
                        "phase",
                        TraceValue::Data(json!(format!("{:?}", self.phase))),
                    ),
                    TraceField::new("event_count", TraceValue::Data(json!(self.events.len()))),
                    TraceField::new(
                        "cancellation_requested",
                        TraceValue::Data(json!(self.cancellation.0.load(Ordering::Relaxed))),
                    ),
                ]
            },
        );
        let cancelled = self.cancellation.is_cancelled();
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!(cancelled)));
        }
        cancelled
    }
}

fn logging_phase(phase: OperationPhase) -> ProgressPhase {
    let trace = trace_current(Component::Core, "operation::logging_phase", || {
        vec![TraceField::new(
            "phase",
            TraceValue::Data(json!(format!("{phase:?}"))),
        )]
    });
    let result = match phase {
        OperationPhase::Preparing => ProgressPhase::Preparing,
        OperationPhase::Processing => ProgressPhase::Processing,
        OperationPhase::Finalizing => ProgressPhase::Finalizing,
    };
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(format!("{result:?}"))));
    }
    result
}

/// Runs an operation and records one terminal event after its progress events.
///
/// At the trace level, successful results are marked as omitted. Production
/// callers with a safely representable result should use
/// [`run_operation_with_trace_result`] and provide an explicit projection.
/// Keeping this function preserves the existing API for callers without a
/// safe projection; their successful trace value remains explicitly omitted.
pub fn run_operation<T>(
    operation: &'static str,
    cancellation: &CancellationToken,
    action: impl FnOnce(&mut OperationContext<'_>) -> Result<T, OperationError>,
) -> OperationRun<T> {
    run_operation_with_trace_result(operation, cancellation, action, |_| {
        TraceValue::Omitted(TraceOmission::UnsupportedType)
    })
}

/// Runs an operation and projects successful results into the full trace.
///
/// The existing [`run_operation`] API remains available for callers that do
/// not have a safe, lossless projection. Production call sites with a safely
/// representable result should use this function and classify sensitive values
/// in their projector. The projector runs only for an active trace session and
/// a successful operation.
pub fn run_operation_with_trace_result<T>(
    operation: &'static str,
    cancellation: &CancellationToken,
    action: impl FnOnce(&mut OperationContext<'_>) -> Result<T, OperationError>,
    project_result: impl FnOnce(&T) -> TraceValue,
) -> OperationRun<T> {
    let trace = trace_current(Component::Core, "operation::run_operation", || {
        vec![
            TraceField::new("operation", TraceValue::Data(json!(operation))),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(cancellation.is_cancelled())),
            ),
            TraceField::new(
                "action",
                TraceValue::Omitted(TraceOmission::UnsupportedType),
            ),
        ]
    });
    let activity = match operation {
        "cli.info" => Activity::CliInfo,
        "build.generate" => Activity::Build,
        operation if operation.starts_with("usb.") => Activity::Usb,
        _ => Activity::OtherOperation,
    };
    let logging_operation = Session::current()
        .and_then(|session| session.start_operation(Component::Core, activity).ok());
    let mut context = OperationContext {
        cancellation,
        phase: OperationPhase::Preparing,
        events: vec![OperationEvent::Started { operation }],
        logging_operation: logging_operation.as_ref(),
    };
    let action_result = match &logging_operation {
        Some(operation) => operation.scoped(|| action(&mut context)),
        None => action(&mut context),
    };
    let (outcome, terminal_event) = match action_result {
        Err(error) => (
            OperationOutcome::Failed(error.clone()),
            OperationEvent::Failed(error),
        ),
        Ok(_) if context.is_cancelled() => (OperationOutcome::Cancelled, OperationEvent::Cancelled),
        Ok(result) => (
            OperationOutcome::Succeeded(result),
            OperationEvent::Succeeded,
        ),
    };
    context.events.push(terminal_event);
    let result = OperationRun {
        outcome,
        events: context.events,
    };
    if let Some(operation) = logging_operation {
        let outcome = match &result.outcome {
            OperationOutcome::Succeeded(_) => Outcome::Succeeded,
            OperationOutcome::Cancelled => Outcome::Cancelled,
            OperationOutcome::Failed(error) => Outcome::FailedWith(match error.code() {
                ErrorCode::Configuration => FailureCategory::Configuration,
                ErrorCode::Privacy => FailureCategory::Privacy,
                ErrorCode::Usb => FailureCategory::Usb,
            }),
        };
        let _ = operation.finish(outcome);
    }
    if let Some(trace) = trace {
        let events = trace_operation_events(&result.events);
        let result_value = match &result.outcome {
            OperationOutcome::Succeeded(value) => json!({
                "outcome": "succeeded",
                "value": project_result(value).to_json(),
                "events": events,
            }),
            OperationOutcome::Failed(error) => json!({
                "outcome": "failed",
                "code": error.code().as_str(),
                "message": trace_sensitive_text(error.message()).to_json(),
                "events": events,
            }),
            OperationOutcome::Cancelled => json!({"outcome": "cancelled", "events": events}),
        };
        let _ = trace.finish(TraceValue::Data(result_value));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        CancellationToken, ErrorCode, OperationError, OperationEvent, OperationOutcome,
        OperationPhase, TraceField, TraceValue, run_operation, run_operation_with_trace_result,
    };
    use serde_json::{Value, json};
    use std::fs;

    fn trace_data(name: &str, value: Value) -> Value {
        json!({"name": name, "content": {"kind": "data", "value": value}})
    }

    fn trace_omission(name: &str, reason: &str) -> Value {
        json!({"name": name, "content": {"kind": "omitted", "reason": reason}})
    }

    fn expected_arguments(fields: Vec<Value>) -> Value {
        Value::Array(fields)
    }

    fn trace_records(path: &std::path::Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .expect("log traccia sintetico")
            .lines()
            .map(|line| serde_json::from_str(line).expect("record JSON valido"))
            .collect()
    }

    fn assert_trace_spans(records: &[Value], function: &str, expected: &[(Value, Value)]) {
        let entries: Vec<_> = records
            .iter()
            .filter(|record| record["type"] == "function_enter" && record["function"] == function)
            .collect();
        assert_eq!(entries.len(), expected.len(), "numero span di {function}");
        for (entry, (arguments, result)) in entries.into_iter().zip(expected) {
            assert_eq!(entry["arguments"], *arguments, "argomenti di {function}");
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
            assert_eq!(exits[0]["result"], *result, "risultato di {function}");
            assert!(
                exits[0]["duration_ms"].as_u64().is_some(),
                "durata mancante per {function}#{call_id}"
            );
        }
    }

    #[test]
    fn operation_error_spans_record_arguments_results_and_formatter_omission() {
        use crate::logging::{Component, Level, Session};

        let directory = std::env::temp_dir().join(format!(
            "sanikey-operation-error-spans-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione trace sintetica");
        const MESSAGE: &str = "errore sintetico";
        let error = session.scoped(|| {
            let error = OperationError::new(ErrorCode::Configuration, MESSAGE);
            assert_eq!(error.message(), MESSAGE);
            assert_eq!(error.to_string(), MESSAGE);
            error
        });
        assert_eq!(error.code(), ErrorCode::Configuration);
        drop(session);

        let records = trace_records(&path);
        let message = json!({
            "format": "text_redacted_known_secret_assignments",
            "value": MESSAGE,
        });
        let self_value = json!({
            "code": "configuration",
            "message": {"kind": "data", "value": message},
        });
        assert_trace_spans(
            &records,
            "operation::OperationError::new",
            &[(
                expected_arguments(vec![
                    trace_data("code", json!("configuration")),
                    trace_data(
                        "message",
                        json!({"format": "text_redacted_known_secret_assignments", "value": MESSAGE}),
                    ),
                ]),
                json!({
                    "kind": "data",
                    "value": {
                        "code": "configuration",
                        "message": {"kind": "data", "value": message},
                    },
                }),
            )],
        );
        assert_trace_spans(
            &records,
            "operation::OperationError::message",
            &[
                (
                    expected_arguments(vec![trace_data("self", self_value.clone())]),
                    json!({"kind": "data", "value": message}),
                ),
                (
                    expected_arguments(vec![trace_data("self", self_value.clone())]),
                    json!({"kind": "data", "value": message}),
                ),
            ],
        );
        assert_trace_spans(
            &records,
            "operation::OperationError::fmt",
            &[(
                expected_arguments(vec![
                    trace_data("self", self_value),
                    trace_omission("formatter", "unsupported_type"),
                ]),
                json!({"kind": "data", "value": {"ok": true}}),
            )],
        );
        fs::remove_file(path).expect("rimozione log sintetico");
        fs::remove_dir(directory).expect("rimozione directory sintetica");
    }

    #[test]
    fn cancellation_token_spans_record_transitions_and_results() {
        use crate::logging::{Component, Level, Session};

        let directory = std::env::temp_dir().join(format!(
            "sanikey-cancellation-spans-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione trace sintetica");
        let token = CancellationToken::default();
        session.scoped(|| {
            assert!(!token.is_cancelled());
            token.cancel();
            assert!(token.is_cancelled());
        });
        drop(session);

        let records = trace_records(&path);
        assert_trace_spans(
            &records,
            "operation::CancellationToken::is_cancelled",
            &[
                (
                    expected_arguments(vec![trace_data("self_cancelled_at_entry", json!(false))]),
                    json!({"kind": "data", "value": false}),
                ),
                (
                    expected_arguments(vec![trace_data("self_cancelled_at_entry", json!(true))]),
                    json!({"kind": "data", "value": true}),
                ),
            ],
        );
        assert_trace_spans(
            &records,
            "operation::CancellationToken::cancel",
            &[(
                expected_arguments(vec![trace_data("previously_cancelled", json!(false))]),
                json!({"kind": "data", "value": null}),
            )],
        );
        fs::remove_file(path).expect("rimozione log sintetico");
        fs::remove_dir(directory).expect("rimozione directory sintetica");
    }

    #[test]
    fn operation_context_and_run_spans_record_arguments_results_and_omissions() {
        use crate::logging::{Component, Level, Session};

        let directory = std::env::temp_dir().join(format!(
            "sanikey-operation-context-spans-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione trace sintetica");
        let run = session.scoped(|| {
            run_operation(
                "synthetic.operation",
                &CancellationToken::default(),
                |context| {
                    context.set_phase(OperationPhase::Processing);
                    context.report_progress(2, Some(5));
                    assert!(!context.is_cancelled());
                    Ok(42)
                },
            )
        });
        assert_eq!(run.outcome, OperationOutcome::Succeeded(42));
        drop(session);

        let records = trace_records(&path);
        let null_result = json!({"kind": "data", "value": null});
        assert_trace_spans(
            &records,
            "operation::OperationContext::set_phase",
            &[(
                expected_arguments(vec![
                    trace_data("previous_phase", json!("Preparing")),
                    trace_data("previous_event_count", json!(1)),
                    trace_data("cancellation_requested", json!(false)),
                    trace_data("phase", json!("Processing")),
                ]),
                null_result.clone(),
            )],
        );
        assert_trace_spans(
            &records,
            "operation::OperationContext::report_progress",
            &[(
                expected_arguments(vec![
                    trace_data("phase", json!("Processing")),
                    trace_data("previous_event_count", json!(1)),
                    trace_data("cancellation_requested", json!(false)),
                    trace_data("completed", json!(2)),
                    trace_data("total", json!(5)),
                ]),
                null_result,
            )],
        );
        let cancelled_result = json!({"kind": "data", "value": false});
        assert_trace_spans(
            &records,
            "operation::OperationContext::is_cancelled",
            &[
                (
                    expected_arguments(vec![
                        trace_data("phase", json!("Processing")),
                        trace_data("event_count", json!(2)),
                        trace_data("cancellation_requested", json!(false)),
                    ]),
                    cancelled_result.clone(),
                ),
                (
                    expected_arguments(vec![
                        trace_data("phase", json!("Processing")),
                        trace_data("event_count", json!(2)),
                        trace_data("cancellation_requested", json!(false)),
                    ]),
                    cancelled_result,
                ),
            ],
        );
        assert_trace_spans(
            &records,
            "operation::run_operation",
            &[(
                expected_arguments(vec![
                    trace_data("operation", json!("synthetic.operation")),
                    trace_data("cancellation_requested", json!(false)),
                    trace_omission("action", "unsupported_type"),
                ]),
                json!({
                    "kind": "data",
                    "value": {
                        "outcome": "succeeded",
                        "value": {"kind": "omitted", "reason": "unsupported_type"},
                        "events": [
                            {"kind": "started", "operation": "synthetic.operation"},
                            {"kind": "progress", "phase": "processing", "completed": 2, "total": 5},
                            {"kind": "succeeded"},
                        ],
                    },
                }),
            )],
        );
        fs::remove_file(path).expect("rimozione log sintetico");
        fs::remove_dir(directory).expect("rimozione directory sintetica");
    }

    #[test]
    fn scoped_trace_records_operation_arguments_and_explicit_omissions() {
        use crate::logging::{Component, Level, Session};
        use std::fs;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-operation-trace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .unwrap();
        let run = session.scoped(|| {
            run_operation("synthetic.operation", &CancellationToken::default(), |_| {
                Ok(42)
            })
        });
        assert_eq!(run.outcome, OperationOutcome::Succeeded(42));
        drop(session);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("operation::run_operation"));
        assert!(contents.contains("synthetic.operation"));
        assert!(contents.contains("unsupported_type"));
        assert!(contents.contains("\"outcome\":\"succeeded\""));
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn trace_records_projected_success_result_and_redacts_known_secret() {
        use crate::logging::{Component, Level, Session, trace_sensitive_text};
        use std::fs;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-operation-projected-result-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .unwrap();
        let result = session.scoped(|| {
            run_operation_with_trace_result(
                "synthetic.operation",
                &CancellationToken::default(),
                |context| {
                    context.set_phase(OperationPhase::Processing);
                    context.report_progress(2, Some(3));
                    Ok("token = \"synthetic-result-secret\"".to_owned())
                },
                |value| {
                    TraceValue::record(&[
                        TraceField::new("kind", TraceValue::data("synthetic_result")),
                        TraceField::new("value", trace_sensitive_text(value)),
                    ])
                },
            )
        });

        assert!(matches!(result.outcome, OperationOutcome::Succeeded(_)));
        drop(session);
        let records = trace_records(&path);
        let operation_result = records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "operation::run_operation"
            })
            .expect("risultato span operazione");
        assert_eq!(operation_result["result"]["kind"], "data");
        assert_eq!(operation_result["result"]["value"]["outcome"], "succeeded");
        assert_eq!(operation_result["result"]["value"]["value"]["kind"], "data");
        let events = operation_result["result"]["value"]["events"]
            .as_array()
            .expect("eventi sintetici serializzati");
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0],
            json!({"kind": "started", "operation": "synthetic.operation"})
        );
        assert_eq!(
            events[1],
            json!({"kind": "progress", "phase": "processing", "completed": 2, "total": 3})
        );
        assert_eq!(events[2], json!({"kind": "succeeded"}));
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("synthetic_result"));
        assert!(contents.contains("secret_omitted"));
        assert!(!contents.contains("synthetic-result-secret"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn trace_redacts_known_secrets_in_operation_errors_without_changing_them() {
        use crate::logging::{Component, Level, Session};
        use std::fs;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-operation-secret-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .unwrap();
        let original = "password = \"synthetic-secret-value\"";
        let run = session.scoped(|| {
            run_operation("synthetic.error", &CancellationToken::default(), |_| {
                Err::<(), _>(OperationError::new(ErrorCode::Configuration, original))
            })
        });
        let OperationOutcome::Failed(error) = run.outcome else {
            panic!("expected synthetic failure");
        };
        assert_eq!(error.message(), original);
        drop(session);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("synthetic-secret-value"));
        assert!(contents.contains("secret_omitted"));
        let records = trace_records(&path);
        let operation_result = records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "operation::run_operation"
            })
            .expect("risultato span errore");
        let events = operation_result["result"]["value"]["events"]
            .as_array()
            .expect("eventi di errore serializzati");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["kind"], "started");
        assert_eq!(events[0]["operation"], "synthetic.error");
        assert_eq!(events[1]["kind"], "failed");
        assert_eq!(events[1]["code"], "configuration");
        assert_eq!(events[1]["message"]["kind"], "data");
        assert_eq!(
            events[1]["message"]["value"]["format"],
            "text_redacted_known_secret_assignments"
        );
        assert!(
            events[1]["message"]["value"]["value"]
                .as_str()
                .unwrap()
                .contains("[secret_omitted]")
        );
        assert!(
            !events[1]["message"]
                .to_string()
                .contains("synthetic-secret-value")
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn trace_progress_uses_event_counts_and_preserves_each_progress_record() {
        use crate::logging::{Component, Level, Session};
        use std::fs;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-operation-progress-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .unwrap();
        session.scoped(|| {
            run_operation(
                "synthetic.operation",
                &CancellationToken::default(),
                |ctx| {
                    for count in 1..=3 {
                        ctx.report_progress(count, Some(3));
                        let _ = ctx.is_cancelled();
                    }
                    Ok(())
                },
            )
        });
        drop(session);
        let log = fs::read_to_string(&path).unwrap();
        let records: Vec<serde_json::Value> = log
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let progress: Vec<_> = records
            .iter()
            .filter(|record| record["type"] == "operation_progress")
            .collect();
        assert_eq!(progress.len(), 3);
        assert_eq!(
            progress
                .iter()
                .map(|record| record["completed"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(log.contains("previous_event_count"));
        assert!(!log.contains("events_debug"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn minimal_events_capture_phase_progress_and_error_category_without_message() {
        use crate::logging::{Component, Level, Session};
        use std::fs;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-operation-events-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Events)
                .unwrap();
        let run = session.scoped(|| {
            run_operation::<()>(
                "synthetic.operation",
                &CancellationToken::default(),
                |context| {
                    context.set_phase(OperationPhase::Processing);
                    context.report_progress(2, Some(3));
                    Err(OperationError::new(
                        ErrorCode::Privacy,
                        "dato sanitario sintetico",
                    ))
                },
            )
        });
        assert!(matches!(run.outcome, OperationOutcome::Failed(_)));
        drop(session);
        let log = fs::read_to_string(&path).unwrap();
        assert!(log.contains("\"type\":\"operation_phase\""));
        assert!(log.contains("\"type\":\"operation_progress\""));
        assert!(log.contains("\"completed\":2"));
        assert!(log.contains("\"failure_category\":\"privacy\""));
        assert!(!log.contains("dato sanitario sintetico"));
        assert!(!log.contains("function_exit"));
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn success_reports_ordered_progress_and_one_terminal_event() {
        let run = run_operation("cli.info", &CancellationToken::default(), |context| {
            context.set_phase(OperationPhase::Processing);
            context.report_progress(1, Some(1));
            Ok("sanikey")
        });

        assert_eq!(run.outcome, OperationOutcome::Succeeded("sanikey"));
        assert_eq!(
            run.events,
            vec![
                OperationEvent::Started {
                    operation: "cli.info"
                },
                OperationEvent::Progress {
                    phase: OperationPhase::Processing,
                    completed: 1,
                    total: Some(1)
                },
                OperationEvent::Succeeded
            ]
        );
    }

    #[test]
    fn failure_keeps_machine_code_separate_from_italian_message() {
        let error = OperationError::new(ErrorCode::Configuration, "configurazione non valida");
        let run = run_operation("config.validate", &CancellationToken::default(), |_| {
            Err(error.clone())
        });

        assert_eq!(run.outcome, OperationOutcome::<()>::Failed(error.clone()));
        assert_eq!(error.code().as_str(), "configuration");
        assert_eq!(error.message(), "configurazione non valida");
        assert_eq!(run.events.last(), Some(&OperationEvent::Failed(error)));
        assert!(!run.events.contains(&OperationEvent::Succeeded));
    }

    #[test]
    fn cancellation_stops_at_a_safe_point_without_success_event() {
        let token = CancellationToken::default();
        let operation_token = token.clone();
        let run = run_operation("scan.documents", &token, move |context| {
            operation_token.cancel();
            assert!(context.is_cancelled());
            Ok(0)
        });

        assert_eq!(run.outcome, OperationOutcome::Cancelled);
        assert_eq!(run.events.last(), Some(&OperationEvent::Cancelled));
        assert!(!run.events.contains(&OperationEvent::Succeeded));
    }

    #[test]
    fn trace_records_cancelled_operation_event_history() {
        use crate::logging::{Component, Level, Session};

        let directory = std::env::temp_dir().join(format!(
            "sanikey-operation-cancelled-trace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .unwrap();
        let cancellation = CancellationToken::default();
        let action_cancellation = cancellation.clone();
        let run = session.scoped(|| {
            run_operation("synthetic.cancelled", &cancellation, |context| {
                context.set_phase(OperationPhase::Finalizing);
                context.report_progress(1, Some(2));
                action_cancellation.cancel();
                assert!(context.is_cancelled());
                Ok(())
            })
        });

        assert_eq!(run.outcome, OperationOutcome::Cancelled);
        drop(session);
        let records = trace_records(&path);
        let operation_result = records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "operation::run_operation"
            })
            .expect("risultato span annullata");
        assert_eq!(operation_result["result"]["value"]["outcome"], "cancelled");
        assert!(
            !operation_result["result"]["value"]
                .as_object()
                .unwrap()
                .contains_key("value")
        );
        let events = operation_result["result"]["value"]["events"]
            .as_array()
            .expect("eventi di annullamento serializzati");
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0],
            json!({"kind": "started", "operation": "synthetic.cancelled"})
        );
        assert_eq!(
            events[1],
            json!({"kind": "progress", "phase": "finalizing", "completed": 1, "total": 2})
        );
        assert_eq!(events[2], json!({"kind": "cancelled"}));
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
