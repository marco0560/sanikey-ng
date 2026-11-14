//! Verifiche sintetiche della traccia dei confini metadata.

use super::common::TestDirectory;
use crate::logging::{Component, Level, Session, trace_current};
use crate::metadata::trace_metadata_result;
use crate::metadata::{load_curated_metadata, prepare_people_metadata};
use crate::operation::{ErrorCode, OperationError};
use std::fs;

#[test]
fn trace_level_records_private_metadata_and_native_path_encoding() {
    let directory = TestDirectory::new();
    directory.write(
        "problems.toml",
        r#"[[problem]]
id = "synthetic-problem"
title = "PRIVATE-SYNTHETIC-METADATA"
unmodeled_private = "SYNTHETIC-UNMODELED-VALUE"
unmodeled_note = "nota\napi_key = \"SYNTHETIC-SECRET-IN-TEXT\""
api_key = "SYNTHETIC-SECRET-DO-NOT-LOG"
"#,
    );
    let log_directory = directory.path().join("logs");
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione trace sintetica");

    let loaded = session
        .scoped(|| load_curated_metadata(directory.path()))
        .expect("metadati sintetici validi");
    assert_eq!(loaded.problems[0].title, "PRIVATE-SYNTHETIC-METADATA");
    session.finish(Component::Core).expect("chiusura sessione");

    let log = fs::read_to_string(log_path).expect("log trace sintetico");
    for function in [
        "metadata::load_curated_metadata",
        "metadata::load_toml",
        "metadata::load_items",
        "metadata::parse_problem",
        "metadata::required_string",
        "metadata::validate_curated_metadata",
        "metadata::validate_unique_ids",
    ] {
        assert!(
            log.contains(&format!("\"function\":\"{function}\"")),
            "{function}"
        );
    }
    assert!(log.contains("PRIVATE-SYNTHETIC-METADATA"));
    assert!(log.contains("SYNTHETIC-UNMODELED-VALUE"));
    assert!(!log.contains("SYNTHETIC-SECRET-DO-NOT-LOG"));
    assert!(!log.contains("SYNTHETIC-SECRET-IN-TEXT"));
    assert!(log.contains("\"secret_omitted\""));
    assert!(log.contains("\"encoding\":\"utf8\""));
    assert!(log.contains(&directory.path().display().to_string()));
}

#[test]
fn events_level_does_not_capture_private_metadata_values() {
    let directory = TestDirectory::new();
    directory.write(
        "problems.toml",
        "[[problem]]\nid = \"synthetic-problem\"\ntitle = \"PRIVATE-SYNTHETIC-METADATA\"\n",
    );
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Events,
    )
    .expect("sessione Events sintetica");

    let loaded = session
        .scoped(|| load_curated_metadata(directory.path()))
        .expect("metadati sintetici validi");
    assert_eq!(loaded.problems[0].id, "synthetic-problem");
    session.finish(Component::Core).expect("chiusura sessione");

    let log = fs::read_to_string(log_path).expect("log Events sintetico");
    assert!(!log.contains("PRIVATE-SYNTHETIC-METADATA"));
    assert!(!log.contains("synthetic-problem"));
}

#[test]
fn trace_level_records_preflight_person_boundary() {
    let directory = TestDirectory::new();
    directory.write(
        "local-data/patient-a/metadata/problems.toml",
        "[[problem]]\nid = \"synthetic-problem\"\ntitle = \"PRIVATE-PREFLIGHT-METADATA\"\n",
    );
    let config_path = directory.path().join("accounts.toml");
    let source = r#"
[global]
config_version = 1

[[person]]
id = "synthetic-person"
display_name = "Persona sintetica"
source_documents = "local-data/patient-a/documents"
metadata_directory = "local-data/patient-a/metadata"
local_build = "local-data/patient-a/generated"
usb_uuid = "SYNTHETIC-UUID"
"#;
    let value: toml::Value = toml::from_str(source).expect("config TOML sintetica");
    let config =
        crate::config::parse_accounts_data(&value, &config_path).expect("config sintetica valida");
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let people = session
        .scoped(|| prepare_people_metadata(&config, directory.path()))
        .expect("preflight sintetico valido");
    assert_eq!(people[0].person_id, "synthetic-person");
    session.finish(Component::Core).expect("chiusura sessione");

    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(log.contains("\"function\":\"metadata::prepare_people_metadata\""));
    assert!(log.contains("PRIVATE-PREFLIGHT-METADATA"));
    assert!(log.contains("synthetic-person"));
    assert!(log.contains("\"encoding\":\"utf8\""));
}

#[test]
fn trace_redacts_known_secret_assignments_in_errors_without_changing_error_text() {
    let directory = TestDirectory::new();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");
    let message = "errore TOML sintetico:\napi_key = \"SECRET-ERROR-SYNTHETIC\"";
    let error = OperationError::new(ErrorCode::Configuration, message);
    let result: Result<(), OperationError> = Err(error.clone());

    session.scoped(|| {
        let trace = trace_current(Component::Core, "metadata::synthetic_error", Vec::new)
            .expect("trace attiva");
        trace
            .finish(trace_metadata_result(&result))
            .expect("scrittura trace");
    });
    assert_eq!(error.message(), message);
    session.finish(Component::Core).expect("chiusura sessione");

    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(!log.contains("SECRET-ERROR-SYNTHETIC"));
    assert!(log.contains("api_key"));
    assert!(log.contains("secret_omitted"));
}

#[test]
fn malformed_toml_error_text_is_unchanged_and_diagnostic_does_not_leak_secret() {
    let directory = TestDirectory::new();
    let secret = "SECRET-MALFORMED-TOML-SYNTHETIC";
    directory.write(
        "problems.toml",
        &format!(
            "[[problem]]\nid = \"synthetic-problem\"\ntitle = \"Sintetico\"\napi_key = \"{secret}\" trailing\n"
        ),
    );
    let expected = load_curated_metadata(directory.path())
        .expect_err("TOML sintetico malformato")
        .message()
        .to_owned();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    let actual = session
        .scoped(|| load_curated_metadata(directory.path()))
        .expect_err("stesso TOML sintetico malformato");
    assert_eq!(actual.message(), expected);
    session.finish(Component::Core).expect("chiusura sessione");

    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(!log.contains(secret));
}

#[cfg(unix)]
#[test]
fn trace_preserves_non_unicode_metadata_path_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = TestDirectory::new();
    let metadata_path = directory
        .path()
        .join(OsString::from_vec(b"metadata-\xff".to_vec()));
    fs::create_dir(&metadata_path).expect("directory metadata con path sintetico non UTF-8");
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione Trace sintetica");

    session
        .scoped(|| load_curated_metadata(&metadata_path))
        .expect("directory metadata vuota valida");
    session.finish(Component::Core).expect("chiusura sessione");

    let log = fs::read_to_string(log_path).expect("log Trace sintetico");
    assert!(log.contains("\"encoding\":\"unix_bytes\""));
    assert!(log.contains("255"));
}
