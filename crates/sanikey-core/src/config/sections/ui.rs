//! UI section parsing.

use super::super::UiConfig;
use super::super::validation::{
    optional_choice, optional_color, optional_float, optional_short_string, reject_unknown,
    table_or_empty,
};
use super::paths::optional_path;
use crate::operation::OperationError;
use std::path::Path;
use toml::Value;

const UI_FIELDS: &[&str] = &[
    "accent_color",
    "background_image",
    "background_opacity",
    "density",
    "default_tab",
    "timeline_order",
    "document_link_mode",
    "subtitle",
];
pub(in crate::config) fn parse_ui(
    value: &Value,
    context: &str,
    base_dir: &Path,
    base: Option<&UiConfig>,
) -> Result<UiConfig, OperationError> {
    let table = table_or_empty(Some(value), context)?;
    reject_unknown(&table, UI_FIELDS, context)?;
    let defaults = base.cloned().unwrap_or_default();
    let background_image = optional_path(
        &table,
        "background_image",
        context,
        base_dir,
        defaults.background_image.as_deref(),
    )?;
    Ok(UiConfig {
        accent_color: optional_color(&table, "accent_color", context, &defaults.accent_color)?,
        density: optional_choice(
            &table,
            "density",
            context,
            &["compact", "comfortable"],
            &defaults.density,
        )?,
        default_tab: optional_choice(
            &table,
            "default_tab",
            context,
            &["advanced", "documents", "timeline", "summary"],
            &defaults.default_tab,
        )?,
        timeline_order: optional_choice(
            &table,
            "timeline_order",
            context,
            &["asc", "desc"],
            &defaults.timeline_order,
        )?,
        document_link_mode: optional_choice(
            &table,
            "document_link_mode",
            context,
            &["usb-relative"],
            &defaults.document_link_mode,
        )?,
        subtitle: optional_short_string(&table, "subtitle", context, &defaults.subtitle)?,
        background_image,
        background_opacity: optional_float(
            &table,
            "background_opacity",
            context,
            defaults.background_opacity,
            0.0,
            1.0,
        )?,
    })
}
