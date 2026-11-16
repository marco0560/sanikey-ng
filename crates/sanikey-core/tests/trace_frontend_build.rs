//! Copertura sintetica delle tracce di generazione frontend e copia dell'immagine.

use sanikey_core::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use sanikey_core::frontend::{FrontendResult, build_frontend};
use sanikey_core::logging::{Component, Level, Session};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

/// Crea e fornisce accesso alla directory sintetica del test.
impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-trace-frontend-build-{}-{}",
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

/// Rimuove la directory sintetica al termine del fixture.
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
fn frontend_build_traces_complete_person_paths_and_omits_copied_image_bytes() {
    let directory = TestDirectory::new();
    let local_build = directory.path().join("generated/person-a");
    let background = directory.path().join("private-background.bin");
    let background_bytes = [0_u8, 255, 17, 42, 0, 128];
    fs::write(&background, background_bytes).expect("immagine sintetica non testuale");

    // Costruisce tutti i valori di default prima di attivare Trace.
    let ui = UiConfig {
        background_image: Some(background.clone()),
        ..UiConfig::default()
    };
    let person = PersonConfig {
        id: "synthetic-person-a".to_owned(),
        display_name: "Persona sintetica A".to_owned(),
        source_documents: directory.path().join("patients/person-a/documents"),
        metadata_directory: directory.path().join("patients/person-a/metadata"),
        local_build: local_build.clone(),
        usb_uuid: "SYNTHETIC-UUID-A".to_owned(),
        enabled: true,
        ui,
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    };
    let web_dir = local_build.join("web");
    let assets = web_dir.join("assets");
    let expected = FrontendResult {
        web_dir: web_dir.clone(),
        index: web_dir.join("index.html"),
        extended_chart: web_dir.join("parameter-chart.html"),
        script: web_dir.join("app.js"),
        stylesheet: web_dir.join("style.css"),
        helper: assets.join("ui-helper.js"),
        material_script: assets.join("material-web.js"),
        material_stylesheet: assets.join("material-web.css"),
        chart_script: assets.join("chart.umd.min.js"),
        chart_license: assets.join("LICENSE.md"),
    };
    let copied_background = assets.join("background.bin");
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let result = session
        .scoped(|| build_frontend(&person))
        .expect("frontend sintetico");
    assert_eq!(result, expected);
    assert_eq!(
        fs::read(&copied_background).expect("immagine copiata invariata"),
        background_bytes
    );
    assert!(expected.index.is_file());
    session.finish(Component::Core).expect("chiusura sessione");

    let trace = records(&log_path);
    let person_projection = json!({
        "id": person.id,
        "display_name": person.display_name,
        "source_documents": trace_path(&person.source_documents),
        "metadata_directory": trace_path(&person.metadata_directory),
        "local_build": trace_path(&person.local_build),
        "usb_uuid": person.usb_uuid,
        "enabled": person.enabled,
        "ui": {
            "accent_color": "#2563eb",
            "density": "comfortable",
            "default_tab": "documents",
            "timeline_order": "desc",
            "document_link_mode": "usb-relative",
            "subtitle": "Archivio sanitario personale",
            "background_image": trace_path(&background),
            "background_opacity": 0.1,
        },
        "search": {
            "dictionary": null,
            "dictionary_terms": [],
            "dictionary_months": [],
            "advanced_index_warning_mb": 25,
        },
        "ingestion": {"exclude_patterns": [], "include_patterns": []},
    });
    let build_result = json!({
        "kind": "data",
        "value": {
            "output": {
                "kind": "data",
                "value": {
                    "web_dir": trace_path(&expected.web_dir),
                    "index": trace_path(&expected.index),
                    "extended_chart": trace_path(&expected.extended_chart),
                    "script": trace_path(&expected.script),
                    "stylesheet": trace_path(&expected.stylesheet),
                    "helper": trace_path(&expected.helper),
                    "material_script": trace_path(&expected.material_script),
                    "material_stylesheet": trace_path(&expected.material_stylesheet),
                    "chart_script": trace_path(&expected.chart_script),
                    "chart_license": trace_path(&expected.chart_license),
                }
            }
        }
    });
    let build_id = assert_span(
        &trace,
        "frontend::build_frontend",
        json!([argument(
            "person",
            json!({"kind": "data", "value": person_projection})
        )]),
        build_result,
    );

    let copy_result = json!({
        "kind": "data",
        "value": {
            "output": {
                "kind": "data",
                "value": {
                    "copy_result": {"state": "succeeded"},
                    "bytes_copied": background_bytes.len(),
                    "transferred_contents": {"kind": "omitted", "reason": "external_component"},
                }
            }
        }
    });
    let copy_id = assert_span(
        &trace,
        "frontend::copy_file",
        json!([
            argument("source", trace_path(&background)),
            argument("destination", trace_path(&copied_background)),
            argument(
                "transferred_contents",
                json!({"kind": "omitted", "reason": "external_component"})
            )
        ]),
        copy_result,
    );
    assert_ne!(build_id, copy_id, "call_id distinti per build e copia");
    let log = fs::read_to_string(&log_path).expect("log leggibile");
    assert!(!log.contains(&background_bytes.map(|byte| byte.to_string()).join(",")));
}
