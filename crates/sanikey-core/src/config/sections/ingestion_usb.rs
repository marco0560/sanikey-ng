//! Ingestion and USB section parsing.

use super::super::validation::{
    optional_bool, optional_choice, optional_nullable_string, optional_positive_integer,
    optional_string_list, reject_unknown, table_or_empty,
};
use super::super::{IngestionConfig, UsbConfig};
use crate::operation::OperationError;
use toml::Value;

const INGESTION_FIELDS: &[&str] = &["exclude_patterns", "include_patterns"];
const USB_FIELDS: &[&str] = &[
    "copy_strategy",
    "min_free_space_mb",
    "usb_uuid",
    "require_exfat",
];

pub(in crate::config) fn parse_ingestion(
    value: &Value,
    context: &str,
    base: Option<&IngestionConfig>,
) -> Result<IngestionConfig, OperationError> {
    let table = table_or_empty(Some(value), context)?;
    reject_unknown(&table, INGESTION_FIELDS, context)?;
    let defaults = base.cloned().unwrap_or_default();
    let mut exclude_patterns = defaults.exclude_patterns;
    exclude_patterns.extend(optional_string_list(&table, "exclude_patterns", context)?);
    let mut include_patterns = defaults.include_patterns;
    include_patterns.extend(optional_string_list(&table, "include_patterns", context)?);
    Ok(IngestionConfig {
        exclude_patterns,
        include_patterns,
    })
}

pub(in crate::config) fn parse_usb(
    value: &Value,
    context: &str,
) -> Result<UsbConfig, OperationError> {
    let table = table_or_empty(Some(value), context)?;
    reject_unknown(&table, USB_FIELDS, context)?;
    let defaults = UsbConfig::default();
    Ok(UsbConfig {
        usb_uuid: optional_nullable_string(&table, "usb_uuid", context)?,
        require_exfat: optional_bool(&table, "require_exfat", context, defaults.require_exfat)?,
        min_free_space_mb: optional_positive_integer(
            &table,
            "min_free_space_mb",
            context,
            defaults.min_free_space_mb,
        )?,
        copy_strategy: optional_choice(
            &table,
            "copy_strategy",
            context,
            &["python", "rsync-preferred"],
            &defaults.copy_strategy,
        )?,
    })
}
