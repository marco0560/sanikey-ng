//! Config tests for paths.

use super::*;

#[test]
fn test_parse_accounts_rejects_invalid_top_level_shapes() {
    for (source, expected) in [
        (
            "global = \"bad\"\nperson = []",
            "[global] deve essere una tabella",
        ),
        (
            "[global]\nconfig_version = \"1\"\nperson = []",
            "[global].config_version deve essere un intero",
        ),
        (
            "[global]\nconfig_version = 1",
            "e' richiesta almeno una voce [[person]]",
        ),
        (
            "person = \"bad\"\n[global]\nconfig_version = 1",
            "e' richiesta almeno una voce [[person]]",
        ),
    ] {
        assert!(
            message(parse_accounts_data(&parse_value(source), "accounts.toml")).contains(expected)
        );
    }
    assert!(
        message(parse_accounts_data(
            &parse_value("person = []"),
            "accounts.toml"
        ))
        .contains("[global].config_version e' obbligatorio")
    );
}

#[test]
fn test_parse_accounts_accepts_repository_version_alias() {
    let directory = TestDirectory::new();
    let source = format!(
        r##"
        [global]
        repository_version = 2
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

    let config = parse_accounts_data(
        &parse_value(&source),
        directory.path().join("accounts.toml"),
    )
    .expect("alias repository_version supportato");
    assert_eq!(config.config_version, 2);
}

#[test]
fn test_parse_accounts_resolves_relative_paths_from_config_root() {
    let directory = TestDirectory::new();
    let source = r##"
        [global]
        config_version = 1
        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "local-data/patient-a/documents"
        metadata_directory = "local-data/patient-a/metadata"
        local_build = "local-data/generated/patient-a"
        usb_uuid = "1A2B-3C4D"
        "##;
    let path = directory.path().join("config/accounts.toml");
    let config = parse_accounts_data(&parse_value(source), &path).expect("percorsi relativi");

    assert_eq!(
        config.people[0].source_documents,
        directory.path().join("local-data/patient-a/documents")
    );
    assert_eq!(
        config.people[0].metadata_directory,
        directory.path().join("local-data/patient-a/metadata")
    );
    assert_eq!(
        config.people[0].local_build,
        directory.path().join("local-data/generated/patient-a")
    );
}

#[test]
fn test_parse_accounts_rejects_invalid_patient_id() {
    let directory = TestDirectory::new();
    let source = accounts_text(directory.path()).replace("patient-a", "Patient A");
    assert!(
        message(parse_accounts_data(
            &parse_value(&source),
            directory.path().join("accounts.toml")
        ))
        .contains("id non valido")
    );
}

#[test]
fn test_parse_accounts_rejects_non_table_person_entry() {
    let source = "person = [\"bad\"]\n[global]\nconfig_version = 1";
    assert!(
        message(parse_accounts_data(&parse_value(source), "accounts.toml"))
            .contains("voce [[person]] 0 deve essere una tabella")
    );
}
