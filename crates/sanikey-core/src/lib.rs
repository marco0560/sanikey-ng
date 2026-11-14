//! Modello delle operazioni e identità applicativa del nucleo SaniKey.

#![forbid(unsafe_code)]

pub mod aifa;
pub mod build;
pub mod config;
pub mod containers;
pub mod database;
pub mod dicom;
pub mod exports;
pub mod frontend;
pub mod inventory;
#[cfg(target_os = "linux")]
pub mod linux_volume;
pub mod logging;
pub mod macos_volume;
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
pub mod usb_copy;
pub mod usb_image;
pub mod vital_signs;
pub mod windows_volume;

use operation::{CancellationToken, OperationRun, run_operation_with_trace_result};

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
    let trace = logging::trace_current(logging::Component::Core, "project_info", Vec::new);
    let result = run_operation_with_trace_result(
        "cli.info",
        &CancellationToken::default(),
        |_| {
            Ok(format!(
                "progetto=sanikey\npackage=sanikey\ncli=sanikey\nversione={}\n",
                version()
            ))
        },
        |value: &String| logging::trace_sensitive_text(value),
    );
    if let Some(trace) = trace {
        let _ = trace.finish(logging::TraceValue::Data(serde_json::json!({
            "format": "rust_debug",
            "value": format!("{result:?}"),
        })));
    }
    result
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

    #[test]
    fn project_info_trace_projects_its_successful_operation_result() {
        use crate::logging::{Component, Level, Session};
        use crate::operation::OperationOutcome;
        use serde_json::Value;
        use std::fs;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-project-info-result-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .unwrap();
        let result = session.scoped(project_info);

        assert!(matches!(
            &result.outcome,
            OperationOutcome::Succeeded(value) if value.contains("package=sanikey")
        ));
        drop(session);
        let records: Vec<Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let operation_result = records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "operation::run_operation"
            })
            .expect("risultato span operazione info");
        assert_eq!(operation_result["result"]["value"]["outcome"], "succeeded");
        assert_eq!(operation_result["result"]["value"]["value"]["kind"], "data");
        assert!(
            operation_result["result"]["value"]["value"]["value"]["value"]
                .as_str()
                .unwrap()
                .contains("package=sanikey")
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
