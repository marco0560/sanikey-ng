//! Modello delle operazioni e identità applicativa del nucleo SaniKey.

#![forbid(unsafe_code)]

pub mod aifa;
pub mod config;
pub mod containers;
pub mod database;
pub mod dicom;
pub mod inventory;
pub mod metadata;
pub mod observation_imports;
mod office;
pub mod operation;
pub mod parameter_reports;
pub mod parameter_rules;
pub mod parameter_slices;
pub mod pdf;
pub mod privacy;
pub mod simulated_export;
pub mod txt_pipeline;
pub mod vital_signs;

use operation::{CancellationToken, OperationRun, run_operation};

/// Returns the version assigned to the SaniKey workspace packages.
///
/// # Examples
///
/// ```
/// assert_eq!(sanikey_core::version(), "0.1.0");
/// ```
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Builds the machine-facing identity output used by the `info` command.
#[must_use]
pub fn project_info() -> OperationRun<String> {
    run_operation("cli.info", &CancellationToken::default(), |_| {
        Ok(format!(
            "progetto=sanikey\npackage=sanikey\ncli=sanikey\nversione={}\n",
            version()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{operation::OperationEvent, project_info, version};

    #[test]
    fn version_matches_workspace_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn project_info_emits_a_successful_operation_lifecycle() {
        let run = project_info();

        assert_eq!(
            run.events,
            vec![
                OperationEvent::Started {
                    operation: "cli.info"
                },
                OperationEvent::Succeeded
            ]
        );
    }
}
