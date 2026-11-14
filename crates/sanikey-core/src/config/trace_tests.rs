//! Synthetic checks for classified configuration traces.

use super::trace::trace_person_paths;
use super::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::logging::{Component, Level, Session};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

fn trace_directory() -> PathBuf {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "sanikey-config-trace-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn parsing_records_private_configuration_only_at_trace_level() {
    let source_path = Path::new("/synthetic/config/accounts.toml");
    let source = r#"
[global]
config_version = 1

[[person]]
id = "patient-synthetic"
display_name = "Persona sintetica"
source_documents = "/synthetic/documents"
metadata_directory = "/synthetic/metadata"
local_build = "/synthetic/build"
usb_uuid = "UUID-SYNTHETIC-1234"
"#;
    let data: toml::Value = toml::from_str(source).expect("TOML sintetico valido");

    for level in [Level::Events, Level::Trace] {
        let directory = trace_directory();
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, level)
                .expect("sessione log sintetica");
        let parsed = session
            .scoped(|| super::parse_accounts_data(&data, source_path))
            .expect("configurazione sintetica valida");
        assert_eq!(parsed.people[0].id, "patient-synthetic");
        session.finish(Component::Core).expect("chiusura log");
        drop(session);

        let contents = fs::read_to_string(&path).expect("log sintetico leggibile");
        if level == Level::Trace {
            assert!(contents.contains("config::parse_accounts_data"));
            assert!(contents.contains("Persona sintetica"));
            assert!(contents.contains("UUID-SYNTHETIC-1234"));
            assert!(contents.contains("unix_bytes") || contents.contains("utf8"));
        } else {
            assert!(!contents.contains("patient-synthetic"));
            assert!(!contents.contains("Persona sintetica"));
            assert!(!contents.contains("UUID-SYNTHETIC-1234"));
        }
        fs::remove_file(path).expect("rimozione log");
        fs::remove_dir(directory).expect("rimozione directory log");
    }
}

#[test]
fn rejected_secret_like_toml_fields_are_redacted_without_losing_other_values() {
    let directory = trace_directory();
    let (session, path) =
        Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
            .expect("sessione log sintetica");
    let source = r#"
[global]
config_version = 1
ordinary = "dato privato sintetico"
password = "secret-config-value"
"#;
    let data: toml::Value = toml::from_str(source).expect("TOML sintetico valido");
    let result = session
        .scoped(|| super::parse_accounts_data(&data, Path::new("/synthetic/config/accounts.toml")));
    assert!(result.is_err());
    session.finish(Component::Core).expect("chiusura log");
    drop(session);

    let contents = fs::read_to_string(&path).expect("log sintetico leggibile");
    assert!(contents.contains("dato privato sintetico"));
    assert!(contents.contains("secret_omitted"));
    assert!(!contents.contains("secret-config-value"));
    fs::remove_file(path).expect("rimozione log");
    fs::remove_dir(directory).expect("rimozione directory log");
}

#[test]
fn enabled_people_redacts_secret_assignments_in_debug_paths_and_keeps_private_fields() {
    let directory = trace_directory();
    let (session, path) =
        Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
            .expect("sessione log sintetica");
    let source = r#"
[global]
config_version = 1

[[person]]
id = "patient-synthetic"
display_name = "Persona privata sintetica"
source_documents = "/synthetic/documents"
metadata_directory = "/synthetic/metadata"
local_build = "/synthetic/generated/api_key=synthetic-config-path-secret"
usb_uuid = "UUID-SYNTHETIC"
"#;
    let data: toml::Value = toml::from_str(source).expect("TOML sintetico valido");
    let config = super::parse_accounts_data(&data, Path::new("/synthetic/accounts.toml"))
        .expect("configurazione sintetica valida");

    let count = session.scoped(|| config.enabled_people().count());
    assert_eq!(count, 1);
    session.finish(Component::Core).expect("chiusura log");
    drop(session);

    let contents = fs::read_to_string(&path).expect("log sintetico leggibile");
    assert!(contents.contains("Persona privata sintetica"));
    assert!(contents.contains("secret_omitted"));
    assert!(!contents.contains("synthetic-config-path-secret"));
    fs::remove_file(path).expect("rimozione log");
    fs::remove_dir(directory).expect("rimozione directory log");
}

#[test]
fn imported_profile_keeps_redacted_source_contents_in_trace() {
    let directory = trace_directory();
    fs::create_dir_all(&directory).expect("directory sintetica");
    let source_path = directory.join("accounts.toml");
    let source = format!(
        r#"
# nota sintetica conservata
# password = "secret-profile-comment"
[global]
config_version = 1

[[person]]
id = "patient-synthetic"
display_name = "Persona sintetica"
source_documents = {:?}
metadata_directory = {:?}
local_build = {:?}
usb_uuid = "UUID-SYNTHETIC-1234"
"#,
        directory.join("documents"),
        directory.join("metadata"),
        directory.join("build"),
    );
    fs::write(&source_path, source).expect("profilo sintetico");
    let log_directory = trace_directory();
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione log sintetica");
    let profile = session
        .scoped(|| super::AccountsProfile::import(&source_path))
        .expect("profilo sintetico valido");
    assert_eq!(profile.accounts().people[0].id, "patient-synthetic");
    session.finish(Component::Core).expect("chiusura log");
    drop(session);

    let contents = fs::read_to_string(&log_path).expect("log sintetico leggibile");
    assert!(contents.contains("nota sintetica conservata"));
    assert!(contents.contains("secret_omitted"));
    assert!(!contents.contains("secret-profile-comment"));
    fs::remove_file(log_path).expect("rimozione log");
    fs::remove_dir(log_directory).expect("rimozione directory log");
    fs::remove_dir_all(directory).expect("rimozione fixture");
}

#[test]
fn malformed_toml_errors_and_debug_results_do_not_reproduce_secret_values() {
    let directory = trace_directory();
    fs::create_dir_all(&directory).expect("directory sintetica");
    let source_path = directory.join("accounts.toml");
    fs::write(
        &source_path,
        r#"[global]
config_version = 1
password = """unclosed-secret-parse-value
"#,
    )
    .expect("TOML malformato sintetico");
    let log_directory = trace_directory();
    let (session, log_path) =
        Session::create_in_directory_with_level(&log_directory, Component::Core, Level::Trace)
            .expect("sessione log sintetica");
    let result = session.scoped(|| super::load_accounts(&source_path));
    assert!(result.is_err());
    session.finish(Component::Core).expect("chiusura log");
    drop(session);

    let contents = fs::read_to_string(&log_path).expect("log sintetico leggibile");
    assert!(!contents.contains("unclosed-secret-parse-value"));
    assert!(contents.contains("TOML non valido"));
    fs::remove_file(log_path).expect("rimozione log");
    fs::remove_dir(log_directory).expect("rimozione directory log");
    fs::remove_dir_all(directory).expect("rimozione fixture");
}

#[cfg(unix)]
#[test]
fn nested_person_paths_preserve_non_unicode_native_bytes() {
    use std::os::unix::ffi::OsStringExt;

    let non_unicode = PathBuf::from(std::ffi::OsString::from_vec(
        b"/synthetic/doc\xffs".to_vec(),
    ));
    let person = PersonConfig {
        id: "patient-synthetic".to_owned(),
        display_name: "Persona sintetica".to_owned(),
        source_documents: non_unicode,
        metadata_directory: PathBuf::from("/synthetic/metadata"),
        local_build: PathBuf::from("/synthetic/build"),
        usb_uuid: "UUID-SYNTHETIC-1234".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    };

    let encoded = trace_person_paths(&person).to_json();
    assert_eq!(
        encoded["value"][0]["path"]["value"]["encoding"],
        "unix_bytes"
    );
    assert_eq!(
        encoded["value"][0]["path"]["value"]["value"],
        serde_json::json!(b"/synthetic/doc\xffs")
    );
}
