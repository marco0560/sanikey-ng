//! Generazione del frontend statico offline per una singola persona.

use crate::config::PersonConfig;
use crate::logging::{
    Component, TraceField, TraceOmission, TraceValue, trace_current, trace_json_value,
    trace_sensitive_text,
};
use crate::operation::{ErrorCode, OperationError};
use crate::version;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

const INDEX_TEMPLATE: &str = include_str!("../assets/frontend/index.html.template");
const EXTENDED_CHART_TEMPLATE: &str =
    include_str!("../assets/frontend/parameter-chart.html.template");
const APP_JS: &str = include_str!("../assets/frontend/app.js");
const PARAMETER_CHART_JS: &str = include_str!("../assets/frontend/parameter-chart.js");
const STYLE_CSS: &str = include_str!("../assets/frontend/style.css");
const CHART_NAVIGATION_JS: &str = include_str!("../assets/frontend/assets/chart-navigation.js");
const MATERIAL_WEB_JS: &str = include_str!("../assets/frontend/assets/material-web.js");
const MATERIAL_WEB_CSS: &str = include_str!("../assets/frontend/assets/material-web.css");
const UI_HELPER_JS: &str = include_str!("../assets/frontend/assets/ui-helper.js");
const CHART_JS: &[u8] = include_bytes!("../assets/frontend/assets/chart.umd.min.js");
const CHART_LICENSE: &[u8] = include_bytes!("../assets/frontend/assets/LICENSE.md");
const SANKEY_LOGO: &[u8] =
    include_bytes!("../assets/frontend/assets/sanikey-logo-horizontal-transparent.svg");

const TEXT_ASSETS: [(&str, &str); 7] = [
    ("app.js", APP_JS),
    ("parameter-chart.js", PARAMETER_CHART_JS),
    ("style.css", STYLE_CSS),
    ("assets/chart-navigation.js", CHART_NAVIGATION_JS),
    ("assets/material-web.js", MATERIAL_WEB_JS),
    ("assets/material-web.css", MATERIAL_WEB_CSS),
    ("assets/ui-helper.js", UI_HELPER_JS),
];

macro_rules! trace_frontend_call {
    ($function:literal, [$($field:expr),* $(,)?], $body:expr, $output:expr) => {{
        let trace = trace_current(Component::Core, $function, || {
            vec![$($field),*]
        });
        let result = $body;
        if let Some(trace) = trace {
            let output = $output(&result);
            let _ = trace.finish(TraceValue::Data(json!({
                "output": output.to_json(),
            })));
        }
        result
    }};
}

fn person_trace(person: &PersonConfig) -> TraceValue {
    trace_json_value(&json!({
        "id": person.id,
        "display_name": person.display_name,
        "source_documents": TraceValue::path(&person.source_documents).to_json(),
        "metadata_directory": TraceValue::path(&person.metadata_directory).to_json(),
        "local_build": TraceValue::path(&person.local_build).to_json(),
        "enabled": person.enabled,
        "ui": {
            "accent_color": person.ui.accent_color,
            "density": person.ui.density,
            "default_tab": person.ui.default_tab,
            "timeline_order": person.ui.timeline_order,
            "document_link_mode": person.ui.document_link_mode,
            "subtitle": person.ui.subtitle,
            "background_image": person.ui.background_image.as_deref().map(|path| TraceValue::path(path).to_json()),
            "background_opacity": person.ui.background_opacity,
        },
        "search": {
            "dictionary": person.search.dictionary.as_deref().map(|path| TraceValue::path(path).to_json()),
            "dictionary_terms": person.search.dictionary_data.terms.iter().collect::<Vec<_>>(),
            "dictionary_months": person.search.dictionary_data.months.iter().collect::<Vec<_>>(),
            "advanced_index_warning_mb": person.search.advanced_index_warning_mb,
        },
        "ingestion": {
            "exclude_patterns": person.ingestion.exclude_patterns,
            "include_patterns": person.ingestion.include_patterns,
        },
    }))
}

fn frontend_result_trace(result: &Result<FrontendResult, OperationError>) -> TraceValue {
    let Some(result) = result.as_ref().ok() else {
        return TraceValue::Data(json!(null));
    };
    TraceValue::Data(json!({
        "web_dir": TraceValue::path(&result.web_dir).to_json(),
        "index": TraceValue::path(&result.index).to_json(),
        "extended_chart": TraceValue::path(&result.extended_chart).to_json(),
        "script": TraceValue::path(&result.script).to_json(),
        "stylesheet": TraceValue::path(&result.stylesheet).to_json(),
        "helper": TraceValue::path(&result.helper).to_json(),
        "material_script": TraceValue::path(&result.material_script).to_json(),
        "material_stylesheet": TraceValue::path(&result.material_stylesheet).to_json(),
        "chart_script": TraceValue::path(&result.chart_script).to_json(),
        "chart_license": TraceValue::path(&result.chart_license).to_json(),
    }))
}

fn trace_operation_result<T>(result: &Result<T, OperationError>) -> Value {
    match result {
        Ok(_) => json!({"state":"succeeded"}),
        Err(error) => json!({
            "state":"failed",
            "code":error.code().as_str(),
            "message":trace_sensitive_text(error.message()).to_json(),
        }),
    }
}

/// Percorsi degli artefatti statici generati per una persona.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendResult {
    /// Directory del frontend, utilizzabile direttamente da `file://`.
    pub web_dir: PathBuf,
    /// Pagina principale della consultazione.
    pub index: PathBuf,
    /// Pagina separata dei grafici estesi.
    pub extended_chart: PathBuf,
    /// JavaScript della pagina principale.
    pub script: PathBuf,
    /// Foglio di stile della consultazione.
    pub stylesheet: PathBuf,
    /// Script locale per i controlli delle sezioni.
    pub helper: PathBuf,
    /// Script locale dei componenti UI.
    pub material_script: PathBuf,
    /// Stili locali dei componenti UI.
    pub material_stylesheet: PathBuf,
    /// Distribuzione vendorizzata di Chart.js.
    pub chart_script: PathBuf,
    /// Licenza della distribuzione vendorizzata di Chart.js.
    pub chart_license: PathBuf,
}

/// Genera il frontend statico e i suoi asset locali per una persona.
///
/// I dati JSON/JavaScript sono prodotti separatamente da `exports`; questa
/// funzione scrive la shell consultabile e non richiede un server o la rete.
///
/// # Errors
///
/// Restituisce un errore se una directory o un artefatto non può essere creato
/// o scritto.
pub fn build_frontend(person: &PersonConfig) -> Result<FrontendResult, OperationError> {
    trace_frontend_call!(
        "frontend::build_frontend",
        [TraceField::new("person", person_trace(person))],
        {
            let web_dir = person.local_build.join("web");
            let assets_dir = web_dir.join("assets");
            create_directory(&assets_dir)?;

            let index = web_dir.join("index.html");
            let extended_chart = web_dir.join("parameter-chart.html");
            let script = web_dir.join("app.js");
            let stylesheet = web_dir.join("style.css");
            let helper = assets_dir.join("ui-helper.js");
            let material_script = assets_dir.join("material-web.js");
            let material_stylesheet = assets_dir.join("material-web.css");
            let chart_navigation = assets_dir.join("chart-navigation.js");
            let chart_script = assets_dir.join("chart.umd.min.js");
            let chart_license = assets_dir.join("LICENSE.md");
            let logo = assets_dir.join("sanikey-logo-horizontal-transparent.svg");

            let display_name = escape_html(&person.display_name);
            let subtitle = escape_html(&person.ui.subtitle);
            let default_tab = escape_html(&person.ui.default_tab);
            let replacements = [
                ("__SANIKEY_DISPLAY_NAME__", display_name.as_str()),
                ("__SANIKEY_SUBTITLE__", subtitle.as_str()),
                ("__SANIKEY_DEFAULT_TAB__", default_tab.as_str()),
            ];
            write_text(&index, &render_template(INDEX_TEMPLATE, &replacements))?;
            write_text(
                &extended_chart,
                &render_template(
                    EXTENDED_CHART_TEMPLATE,
                    &[("__SANIKEY_DISPLAY_NAME__", display_name.as_str())],
                ),
            )?;

            for (relative_path, contents) in TEXT_ASSETS {
                write_text(&web_dir.join(relative_path), contents)?;
            }
            write_text(&chart_navigation, CHART_NAVIGATION_JS)?;
            write_bytes(&chart_script, CHART_JS)?;
            write_bytes(&chart_license, CHART_LICENSE)?;
            write_bytes(&logo, SANKEY_LOGO)?;
            write_text(
                &web_dir.join("usb-info.js"),
                &format!(
                    "window.SANIKEY_USB_INFO = {};\n",
                    json!({
                        "schema_version": 1,
                        "usb_uuid": person.usb_uuid,
                        "exported_at": null,
                        "sanikey_version": version(),
                        "copy_strategy": null,
                    })
                ),
            )?;

            if let Some(background) = &person.ui.background_image {
                let extension = background
                    .extension()
                    .map(|value| value.to_string_lossy().to_ascii_lowercase())
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(".{value}"))
                    .unwrap_or_default();
                copy_file(
                    background,
                    &assets_dir.join(format!("background{extension}")),
                )?;
            }

            Ok(FrontendResult {
                web_dir,
                index,
                extended_chart,
                script,
                stylesheet,
                helper,
                material_script,
                material_stylesheet,
                chart_script,
                chart_license,
            })
        },
        frontend_result_trace
    )
}

fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    trace_frontend_call!(
        "frontend::render_template",
        [
            TraceField::new("template", TraceValue::data(template)),
            TraceField::new("replacements", trace_json_value(&json!(replacements))),
        ],
        {
            let mut rendered = String::with_capacity(template.len());
            let mut remaining = template;
            while !remaining.is_empty() {
                let mut next: Option<(usize, &str, &str)> = None;
                for (token, value) in replacements {
                    if let Some(position) = remaining.find(token)
                        && next
                            .as_ref()
                            .is_none_or(|(current, _, _)| position < *current)
                    {
                        next = Some((position, *token, *value));
                    }
                }
                let Some((position, token, value)) = next else {
                    rendered.push_str(remaining);
                    break;
                };
                rendered.push_str(&remaining[..position]);
                rendered.push_str(value);
                remaining = &remaining[position + token.len()..];
            }
            rendered
        },
        |result: &String| trace_sensitive_text(result)
    )
}

fn escape_html(value: &str) -> String {
    trace_frontend_call!(
        "frontend::escape_html",
        [TraceField::new("value", trace_sensitive_text(value))],
        {
            let mut escaped = String::with_capacity(value.len());
            for character in value.chars() {
                escaped.push_str(match character {
                    '&' => "&amp;",
                    '<' => "&lt;",
                    '>' => "&gt;",
                    '"' => "&quot;",
                    '\'' => "&#x27;",
                    _ => {
                        escaped.push(character);
                        continue;
                    }
                });
            }
            escaped
        },
        |result: &String| trace_sensitive_text(result)
    )
}

fn create_directory(path: &Path) -> Result<(), OperationError> {
    trace_frontend_call!(
        "frontend::create_directory",
        [TraceField::new("path", TraceValue::path(path))],
        fs::create_dir_all(path).map_err(|cause| filesystem_error(
            "impossibile creare la directory frontend",
            path,
            cause,
        )),
        |result: &Result<(), OperationError>| TraceValue::Data(trace_operation_result(result))
    )
}

fn write_text(path: &Path, contents: &str) -> Result<(), OperationError> {
    trace_frontend_call!(
        "frontend::write_text",
        [
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("contents", trace_sensitive_text(contents)),
        ],
        {
            if let Some(parent) = path.parent() {
                create_directory(parent)?;
            }
            fs::write(path, contents).map_err(|cause| {
                filesystem_error("impossibile scrivere l'artefatto frontend", path, cause)
            })
        },
        |result: &Result<(), OperationError>| TraceValue::Data(trace_operation_result(result))
    )
}

fn write_bytes(path: &Path, contents: &[u8]) -> Result<(), OperationError> {
    trace_frontend_call!(
        "frontend::write_bytes",
        [
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("contents", trace_frontend_bytes(contents)),
        ],
        {
            if let Some(parent) = path.parent() {
                create_directory(parent)?;
            }
            fs::write(path, contents).map_err(|cause| {
                filesystem_error("impossibile copiare l'asset frontend", path, cause)
            })
        },
        |result: &Result<(), OperationError>| TraceValue::Data(trace_operation_result(result))
    )
}

fn trace_frontend_bytes(contents: &[u8]) -> TraceValue {
    match std::str::from_utf8(contents) {
        Ok(text) => trace_sensitive_text(text),
        Err(_) => TraceValue::Omitted(TraceOmission::UnsupportedType),
    }
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), OperationError> {
    let mut copied_bytes = None;
    trace_frontend_call!(
        "frontend::copy_file",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("destination", TraceValue::path(destination)),
            TraceField::new(
                "transferred_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        match fs::copy(source, destination) {
            Ok(byte_count) => {
                copied_bytes = Some(byte_count);
                Ok(())
            }
            Err(cause) => Err(filesystem_error(
                "impossibile copiare l'immagine di sfondo frontend",
                destination,
                cause,
            )),
        },
        |result: &Result<(), OperationError>| TraceValue::Data(json!({
            "copy_result": trace_operation_result(result),
            "bytes_copied": copied_bytes,
            "transferred_contents": TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
        }))
    )
}

fn filesystem_error(context: &str, path: &Path, cause: std::io::Error) -> OperationError {
    let error_data = json!({
        "kind": format!("{:?}", cause.kind()),
        "raw_os_error": cause.raw_os_error(),
        "message": trace_sensitive_text(&cause.to_string()).to_json(),
    });
    trace_frontend_call!(
        "frontend::filesystem_error",
        [
            TraceField::new("context", TraceValue::data(context)),
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("io_error", TraceValue::Data(error_data)),
        ],
        OperationError::new(
            ErrorCode::Configuration,
            format!("{context} {}: {cause}", path.display()),
        ),
        |result: &OperationError| trace_sensitive_text(result.message())
    )
}

#[cfg(test)]
mod tests;
