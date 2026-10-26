//! Identical synthetic operation events shared by the GUI comparison apps.

use sanikey_core::operation::{
    CancellationToken, ErrorCode, OperationError, OperationEvent, OperationPhase, run_operation,
};

pub use sanikey_core::operation::OperationEvent as GuiOperationEvent;

/// The terminal path to display in both GUI prototypes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemoScenario {
    /// Complete successfully after five progress updates.
    Success,
    /// Return a categorized configuration error after five updates.
    Error,
    /// Request cancellation after the second update.
    Cancel,
}

/// Creates the same deterministic event sequence for both GUI prototypes.
#[must_use]
pub fn demonstration_events(scenario: DemoScenario) -> Vec<OperationEvent> {
    let cancellation = CancellationToken::default();
    let action_cancellation = cancellation.clone();

    run_operation("prototype.generation", &cancellation, move |context| {
        context.set_phase(OperationPhase::Processing);
        context.report_progress(0, None);
        let last_step = if scenario == DemoScenario::Cancel {
            2
        } else {
            5
        };
        for completed in 1..=last_step {
            context.report_progress(completed, Some(5));
            if scenario == DemoScenario::Cancel && completed == last_step {
                action_cancellation.cancel();
            }
        }

        if scenario == DemoScenario::Error {
            Err(OperationError::new(
                ErrorCode::Configuration,
                "configurazione non valida",
            ))
        } else {
            Ok(())
        }
    })
    .events
}

#[cfg(test)]
mod tests {
    use super::{DemoScenario, demonstration_events};
    use sanikey_core::operation::OperationEvent;

    #[test]
    fn scenarios_share_the_core_lifecycle_and_expected_terminal_events() {
        assert!(matches!(
            demonstration_events(DemoScenario::Success).last(),
            Some(OperationEvent::Succeeded)
        ));
        assert!(matches!(
            demonstration_events(DemoScenario::Error).last(),
            Some(OperationEvent::Failed(error)) if error.code().as_str() == "configuration"
        ));
        assert!(matches!(
            demonstration_events(DemoScenario::Cancel).last(),
            Some(OperationEvent::Cancelled)
        ));
    }
}
