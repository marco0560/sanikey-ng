//! Config tests for nested.

use super::*;

#[test]
fn test_parse_accounts_ignores_disabled_people_for_usb_uuid_conflicts() {
    let directory = TestDirectory::new();
    let mut source = accounts_text(directory.path()).replace(
        "usb_uuid = \"1A2B-3C4D\"",
        "usb_uuid = \"1A2B-3C4D\"\nenabled = false",
    );
    source.push_str(&format!(
        r##"

        [[person]]
        id = "patient-b"
        display_name = "Patient B"
        source_documents = "{}"
        metadata_directory = "{}"
        local_build = "{}"
        usb_uuid = "DIFFERENT"
        enabled = false
        "##,
        directory.path().join("source-b").display(),
        directory.path().join("metadata-b").display(),
        directory.path().join("generated-b").display(),
    ));
    let config = parse_accounts_data(
        &parse_value(&source),
        directory.path().join("accounts.toml"),
    )
    .expect("UUID non compatibili di persone disabilitate ignorati");
    assert_eq!(config.enabled_people().count(), 0);
}

#[test]
fn test_parse_accounts_rejects_global_usb_uuid_conflict() {
    let directory = TestDirectory::new();
    let source = accounts_text(directory.path()).replace(
        "config_version = 1",
        "config_version = 1\n[global.usb]\nusb_uuid = \"OTHER\"",
    );
    assert!(
        message(parse_accounts_data(
            &parse_value(&source),
            directory.path().join("accounts.toml")
        ))
        .contains("e' in conflitto con i valori usb_uuid")
    );
}

#[test]
fn test_parse_accounts_expands_current_user_home_paths() {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .expect("directory home disponibile");
    assert_eq!(
        super::super::validation::expand_user_path("~/sanikey-data"),
        home.join("sanikey-data")
    );
}
