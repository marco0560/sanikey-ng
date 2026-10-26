//! Typed operation outcomes, progress events, and cooperative cancellation.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

impl OperationError {
    /// Creates an error without coupling its machine category to its wording.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the machine-readable category.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// Returns the Italian message intended for people.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

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

impl CancellationToken {
    /// Requests cancellation; the operation observes it at its next safe point.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
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
}

impl OperationContext<'_> {
    /// Changes the phase used by subsequent progress events.
    pub fn set_phase(&mut self, phase: OperationPhase) {
        self.phase = phase;
    }

    /// Reports completed work without exposing presentation-specific text.
    pub fn report_progress(&mut self, completed: u64, total: Option<u64>) {
        self.events.push(OperationEvent::Progress {
            phase: self.phase,
            completed,
            total,
        });
    }

    /// Checks cancellation at an operation-defined safe point.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

/// Runs an operation and records one terminal event after its progress events.
pub fn run_operation<T>(
    operation: &'static str,
    cancellation: &CancellationToken,
    action: impl FnOnce(&mut OperationContext<'_>) -> Result<T, OperationError>,
) -> OperationRun<T> {
    let mut context = OperationContext {
        cancellation,
        phase: OperationPhase::Preparing,
        events: vec![OperationEvent::Started { operation }],
    };
    let action_result = action(&mut context);
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
    OperationRun {
        outcome,
        events: context.events,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CancellationToken, ErrorCode, OperationError, OperationEvent, OperationOutcome,
        OperationPhase, run_operation,
    };

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
}
