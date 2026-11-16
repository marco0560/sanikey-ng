//! Verifica le span Trace dei due writer di report macchina dei parametri.

use sanikey_core::logging::{Component, Level, Session};
use sanikey_core::parameter_reports::{
    write_parameter_extraction_reports, write_parameter_reports,
};
use sanikey_core::parameter_rules::ParameterBuildResult;
use sanikey_core::parameter_slices::DiscoveryResult;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Creates the synthetic workspace used by the test.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-parameter-report-writers-{}-{}",
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

/// Removes the synthetic workspace when the test finishes.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn trace_path(path: &Path) -> Value {
    json!({
        "kind":"data",
        "value":{"encoding":"utf8", "value":path.to_str().expect("path UTF-8")}
    })
}

fn trace_debug(value: &str) -> Value {
    json!({"kind":"data", "value":{"format":"rust_debug", "value":value}})
}

fn argument(name: &str, content: Value) -> Value {
    json!({"name":name, "content":content})
}

fn assert_span(
    records: &[Value],
    function: &str,
    expected_arguments: Value,
    expected_result: Value,
) -> u64 {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_enter"
                && record["function"] == function
                && record["parent_call_id"].is_null()
        })
        .collect();
    assert_eq!(entries.len(), 1, "una span radice per {function}");
    let entry = entries[0];
    assert_eq!(entry["component"], "core");
    assert_eq!(entry["arguments"], expected_arguments, "input {function}");
    let call_id = entry["call_id"].as_u64().expect("call_id numerico");
    let exits: Vec<_> = records
        .iter()
        .filter(|record| {
            record["type"] == "function_exit"
                && record["function"] == function
                && record["call_id"] == call_id
        })
        .collect();
    assert_eq!(
        exits.len(),
        1,
        "una uscita correlata per {function}#{call_id}"
    );
    assert_eq!(exits[0]["outcome"], "returned");
    assert!(exits[0]["duration_ms"].as_u64().is_some());
    assert_eq!(exits[0]["result"], expected_result, "output {function}");
    call_id
}

#[test]
fn parameter_report_writers_trace_inputs_results_call_ids_and_durations() {
    let directory = TestDirectory::new();
    let build_root = directory.path().join("generated/person-synthetic");
    let discovery = DiscoveryResult {
        candidates: Vec::new(),
        proposed_groups: Vec::new(),
        analyzed_lines: 0,
    };
    let build_result = ParameterBuildResult {
        series: Vec::new(),
        points: Vec::new(),
        decisions: Vec::new(),
    };
    let report_paths = sanikey_core::parameter_reports::ParameterReportPaths {
        candidates: build_root.join("reports/parameter-candidates.json"),
        proposed_rules: build_root.join("reports/parameter-rules.proposed.toml"),
        slices_json: None,
        slices_javascript: None,
    };
    let extraction_paths = (
        build_root.join("exports/parameter-slices.json"),
        build_root.join("web/data/parameter-slices.js"),
    );

    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");
    let actual_paths = session
        .scoped(|| write_parameter_reports(&build_root, &discovery, None))
        .expect("report parametri sintetico");
    assert_eq!(actual_paths, report_paths);
    let actual_extraction_paths = session
        .scoped(|| write_parameter_extraction_reports(&build_root, &build_result))
        .expect("export parametri sintetici");
    assert_eq!(actual_extraction_paths, extraction_paths);
    session.finish(Component::Core).expect("chiusura Trace");
    drop(session);

    let report_json: Value = serde_json::from_slice(
        &fs::read(&report_paths.candidates).expect("JSON candidati scritto"),
    )
    .expect("JSON candidati valido");
    assert_eq!(
        report_json,
        json!({
            "schema_version":"1.0",
            "analyzed_lines":0,
            "candidates":[],
            "groups":[]
        })
    );
    let proposed_rules =
        fs::read_to_string(&report_paths.proposed_rules).expect("proposta TOML scritta");
    let proposed_rules: toml::Value = toml::from_str(&proposed_rules).expect("TOML valido");
    assert!(proposed_rules.get("generated_at").is_some());
    let extraction_json: Value =
        serde_json::from_slice(&fs::read(&extraction_paths.0).expect("JSON serie scritto"))
            .expect("JSON serie valido");
    assert_eq!(
        extraction_json,
        json!({"schema_version":"1.0", "series":[], "points":[], "decisions":[]})
    );
    assert_eq!(
        fs::read_to_string(&extraction_paths.1).expect("JavaScript scritto"),
        "window.SANIKEY_PARAMETER_SLICES = {\"decisions\":[],\"points\":[],\"schema_version\":\"1.0\",\"series\":[]};\n"
    );

    let records: Vec<Value> = fs::read_to_string(log_path)
        .expect("log Trace sintetico")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect();
    let report_args = json!([
        argument("build_root", trace_path(&build_root)),
        argument("discovery", trace_debug(&format!("{discovery:#?}"))),
        argument("result", json!({"kind":"data", "value":null}))
    ]);
    let expected_report_value = format!(
        "{:#?}",
        Ok::<_, sanikey_core::operation::OperationError>(report_paths.clone())
    );
    let report_result = json!({
        "kind":"data",
        "value":{
            "state":"complete",
            "value":trace_debug(&expected_report_value),
            "paths":{
                "candidates":trace_path(&report_paths.candidates),
                "proposed_rules":trace_path(&report_paths.proposed_rules),
                "slices_json":null,
                "slices_javascript":null
            }
        }
    });
    let report_id = assert_span(
        &records,
        "parameter_reports::write_parameter_reports",
        report_args,
        report_result,
    );

    let extraction_args = json!([
        argument("result", trace_debug(&format!("{build_result:#?}"))),
        argument("build_root", trace_path(&build_root))
    ]);
    let expected_extraction_value = format!(
        "{:#?}",
        Ok::<_, sanikey_core::operation::OperationError>(extraction_paths.clone())
    );
    let extraction_result = json!({
        "kind":"data",
        "value":{
            "state":"complete",
            "value":trace_debug(&expected_extraction_value),
            "paths":{
                "first":trace_path(&extraction_paths.0),
                "second":trace_path(&extraction_paths.1)
            }
        }
    });
    let extraction_id = assert_span(
        &records,
        "parameter_reports::write_parameter_extraction_reports",
        extraction_args,
        extraction_result,
    );
    assert_ne!(report_id, extraction_id);
}
