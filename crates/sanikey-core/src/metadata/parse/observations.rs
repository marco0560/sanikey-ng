//! Parser per serie e punti longitudinali.

use super::super::models::{ObservationPoint, ObservationSeries};
use super::super::value::{
    optional_bool, optional_float, optional_string, required_string, string_tuple,
};
use crate::operation::OperationError;
use std::path::Path;
use toml::Table;

pub(in crate::metadata) fn parse_observation_series(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<ObservationSeries, OperationError> {
    Ok(ObservationSeries {
        id: required_string(item, "id", path, index)?,
        name: required_string(item, "name", path, index)?,
        value_type: required_string(item, "value_type", path, index)?,
        unit: optional_string(item, "unit")?,
        description: optional_string(item, "description")?,
        warn_duplicate_same_day: optional_bool(item, "warn_duplicate_same_day", true, path, index)?,
        synonyms: string_tuple(item.get("synonyms"), path, "synonyms", index)?,
        parameter_rule_id: None,
        parameter_rule_version: None,
        parameter_rule_digest: None,
        unit_variant: None,
    })
}

pub(in crate::metadata) fn parse_observation_point(
    item: &Table,
    path: &Path,
    index: usize,
) -> Result<ObservationPoint, OperationError> {
    Ok(ObservationPoint {
        id: required_string(item, "id", path, index)?,
        series_id: required_string(item, "series_id", path, index)?,
        observation_date: required_string(item, "observation_date", path, index)?,
        source_type: required_string(item, "source_type", path, index)?,
        source_reference: required_string(item, "source_reference", path, index)?,
        numeric_value: optional_float(item, "numeric_value", path, index)?,
        text_value: optional_string(item, "text_value")?,
        systolic: optional_float(item, "systolic", path, index)?,
        diastolic: optional_float(item, "diastolic", path, index)?,
        pulse: optional_float(item, "pulse", path, index)?,
        note: optional_string(item, "note")?,
        source_kind: "curated-observation".to_owned(),
        document_id: None,
        document_href: None,
        document_title: None,
        document_category: None,
        source_text_digest: None,
        original_line: None,
        line_number: None,
        page_number: None,
        character_start: None,
        character_end: None,
        matched_label: None,
        raw_value: None,
        parsed_value: None,
        raw_unit: None,
        normalized_unit: None,
        qualifier: None,
        rule_id: None,
        rule_version: None,
        rule_digest: None,
        reason_code: None,
    })
}
