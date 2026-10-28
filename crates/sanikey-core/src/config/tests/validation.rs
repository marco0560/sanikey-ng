//! Config tests for validation.

use super::*;

#[test]
fn test_parse_accounts_uses_global_usb_uuid_default() {
    let directory = TestDirectory::new();
    let source = format!(
        r##"
        [global]
        config_version = 1
        [global.usb]
        usb_uuid = "757F-7873"
        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "{}"
        metadata_directory = "{}"
        local_build = "{}"
        "##,
        directory.path().join("source").display(),
        directory.path().join("metadata").display(),
        directory.path().join("generated").display(),
    );

    let config = parse_accounts_data(
        &parse_value(&source),
        directory.path().join("accounts.toml"),
    )
    .expect("UUID globale applicato");

    assert_eq!(config.people[0].usb_uuid, "757F-7873");
}

#[test]
fn test_parse_accounts_rejects_missing_usb_uuid_without_global() {
    let directory = TestDirectory::new();
    let source = format!(
        r##"
        [global]
        config_version = 1
        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "{}"
        metadata_directory = "{}"
        local_build = "{}"
        "##,
        directory.path().join("source").display(),
        directory.path().join("metadata").display(),
        directory.path().join("generated").display(),
    );

    assert!(
        message(parse_accounts_data(
            &parse_value(&source),
            directory.path().join("accounts.toml")
        ))
        .contains("usb_uuid or [global.usb].usb_uuid")
    );
}

#[test]
fn test_load_accounts_rejects_invalid_ui_values() {
    let directory = TestDirectory::new();
    let cases = [
        ("accent_color", "\"blue\"", "colore #rrggbb"),
        ("density", "\"wide\"", "uno tra"),
        ("default_tab", "\"search\"", "uno tra"),
        ("timeline_order", "\"newest\"", "uno tra"),
        ("document_link_mode", "\"absolute\"", "uno tra"),
        ("background_opacity", "1.5", "deve essere tra"),
    ];
    for (field, value, expected) in cases {
        let source = format!(
            r##"
            [global]
            config_version = 1
            [global.ui]
            {field} = {value}
            [[person]]
            id = "patient-a"
            display_name = "Patient A"
            source_documents = "{}"
            metadata_directory = "{}"
            local_build = "{}"
            usb_uuid = "1A2B-3C4D"
            "##,
            directory.path().join("source").display(),
            directory.path().join("metadata").display(),
            directory.path().join("generated").display(),
        );
        let path = directory.path().join(format!("{field}.toml"));
        fs::write(&path, source).expect("fixture UI");
        assert!(message(load_accounts(path)).contains(expected), "{field}");
    }
}

#[test]
fn test_load_accounts_rejects_unknown_ui_field() {
    let directory = TestDirectory::new();
    let source = accounts_text(directory.path()).replace(
        "config_version = 1",
        "config_version = 1\n[global.ui]\ncustom_css = \"theme.css\"",
    );
    let path = directory.accounts_file(&source);

    assert!(message(load_accounts(path)).contains("campi sconosciuti: custom_css"));
}

#[test]
fn test_default_accounts_path_uses_current_working_directory() {
    let directory = TestDirectory::new();
    let _lock = CURRENT_DIRECTORY_LOCK
        .lock()
        .expect("lock directory corrente");
    let _working_directory = WorkingDirectory::enter(directory.path());
    assert_eq!(
        default_accounts_path(None).unwrap(),
        directory.path().join("config/accounts.toml")
    );
}

#[test]
fn test_load_accounts_rejects_missing_file() {
    let directory = TestDirectory::new();
    assert!(
        message(load_accounts(directory.path().join("missing.toml")))
            .contains("configurazione account non trovata")
    );
}

#[test]
fn test_load_accounts_rejects_invalid_toml() {
    let directory = TestDirectory::new();
    let path = directory.accounts_file("[global\n");
    assert!(message(load_accounts(path)).contains("TOML non valido"));
}

#[test]
fn test_load_accounts_resolves_relative_config_path() {
    let directory = TestDirectory::new();
    let _lock = CURRENT_DIRECTORY_LOCK
        .lock()
        .expect("lock directory corrente");
    let _working_directory = WorkingDirectory::enter(directory.path());
    let config_dir = directory.path().join("config");
    fs::create_dir(&config_dir).expect("directory config");
    let config_path = config_dir.join("accounts.toml");
    fs::write(
        &config_path,
        r##"
        [global]
        config_version = 1
        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "local-data/patient-a/documents"
        metadata_directory = "local-data/patient-a/metadata"
        local_build = "local-data/generated/patient-a"
        usb_uuid = "1A2B-3C4D"
        "##,
    )
    .expect("file account con percorsi relativi");

    let config = load_accounts(Path::new("config/accounts.toml")).expect("file account caricato");

    assert_eq!(config.path, fs::canonicalize(config_path).unwrap());
    assert_eq!(
        config.people[0].source_documents,
        directory.path().join("local-data/patient-a/documents")
    );
}

#[test]
fn test_parse_accounts_rejects_missing_real_paths() {
    let source = "[global]\nconfig_version = 1\n[[person]]\nid = \"patient-a\"\ndisplay_name = \"Patient A\"\n";
    assert!(
        message(parse_accounts_data(&parse_value(source), "accounts.toml"))
            .contains("campi mancanti")
    );
}
