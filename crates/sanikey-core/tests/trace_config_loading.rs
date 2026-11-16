//! Copertura Trace sintetica per il calcolo del percorso e il caricamento TOML account.

use sanikey_core::config::{default_accounts_path, load_accounts, parse_accounts_data};
use sanikey_core::logging::{Component, Level, Session};
use sanikey_core::operation::ErrorCode;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Gestisce i file temporanei sintetici usati dai test di caricamento configurazione.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-config-loading-{}-{}",
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

/// Rimuove i file temporanei creati dalla fixture di test.
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

fn trace_path_argument(path: &Path) -> Value {
    json!({"kind": "data", "value": trace_path(path)})
}

fn argument(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn error_debug(code: ErrorCode, message: &str) -> String {
    format!(
        "Err(\n    OperationError {{\n        code: {code:?},\n        message: {message:?},\n    }},\n)"
    )
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
fn config_loading_traces_exact_paths_parser_input_and_error_results() {
    let directory = TestDirectory::new();
    let repo_root = directory.path().join("repo");
    let config_path = directory.path().join("config").join("accounts.toml");
    let missing_path = directory.path().join("missing.toml");
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let default_path = session
        .scoped(|| default_accounts_path(Some(&repo_root)))
        .expect("percorso account predefinito");
    assert_eq!(default_path, repo_root.join("config/accounts.toml"));

    let load_error = session
        .scoped(|| load_accounts(&missing_path))
        .expect_err("file sintetico assente");
    assert_eq!(load_error.code(), ErrorCode::Configuration);

    let parse_error = session
        .scoped(|| parse_accounts_data(&toml::Value::String("synthetic-root".into()), &config_path))
        .expect_err("root TOML sintetica non tabella");
    assert_eq!(parse_error.code(), ErrorCode::Configuration);
    assert_eq!(
        parse_error.message(),
        "configurazione deve essere una tabella"
    );
    session.finish(Component::Core).expect("chiusura sessione");

    let trace = records(&log_path);
    let default_id = assert_span(
        &trace,
        "config::default_accounts_path",
        json!([
            argument(
                "repo_root",
                json!({
                    "kind": "data",
                    "value": {
                        "format": "rust_debug",
                        "value": format!("{:#?}", Some(repo_root.as_path()))
                    }
                })
            ),
            argument("repo_root_native_path", trace_path_argument(&repo_root))
        ]),
        json!({
            "kind": "data",
            "value": {
                "format": "rust_debug",
                "value": format!("{:#?}", Ok::<_, ErrorCode>(default_path.clone())),
                "native_paths": {
                    "kind": "data",
                    "value": {"state": "complete", "path": trace_path(&default_path)}
                }
            }
        }),
    );
    let load_id = assert_span(
        &trace,
        "config::load_accounts",
        json!([argument("path", trace_path(&missing_path))]),
        json!({
            "kind": "data",
            "value": {
                "format": "rust_debug",
                "value": error_debug(
                    ErrorCode::Configuration,
                    &format!("configurazione account non trovata: {}", missing_path.display())
                ),
                "native_paths": {"kind": "data", "value": {"state": "error", "paths": []}}
            }
        }),
    );
    let parse_id = assert_span(
        &trace,
        "config::parse_accounts_data",
        json!([
            argument("path", trace_path(&config_path)),
            argument(
                "data",
                json!({"kind": "data", "value": {"type": "string", "value": "synthetic-root"}})
            )
        ]),
        json!({
            "kind": "data",
            "value": {
                "format": "rust_debug",
                "value": error_debug(
                    ErrorCode::Configuration,
                    "configurazione deve essere una tabella"
                ),
                "native_paths": {"kind": "data", "value": {"state": "error", "paths": []}}
            }
        }),
    );
    let mut call_ids = vec![default_id, load_id, parse_id];
    call_ids.sort_unstable();
    call_ids.dedup();
    assert_eq!(call_ids.len(), 3, "call_id distinti per le tre span attese");
}
