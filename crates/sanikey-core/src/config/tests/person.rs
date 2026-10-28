//! Config tests for person.

use super::*;

#[test]
fn test_parse_accounts_rejects_non_boolean_enabled() {
    let directory = TestDirectory::new();
    let source = accounts_text(directory.path()).replace(
        "usb_uuid = \"1A2B-3C4D\"",
        "usb_uuid = \"1A2B-3C4D\"\nenabled = \"yes\"",
    );
    assert!(
        message(parse_accounts_data(
            &parse_value(&source),
            directory.path().join("accounts.toml")
        ))
        .contains("campo enabled deve essere booleano")
    );
}

#[test]
fn test_parse_accounts_rejects_empty_string_fields() {
    let directory = TestDirectory::new();
    let source = accounts_text(directory.path()).replace("Patient A", "  ");
    assert!(
        message(parse_accounts_data(
            &parse_value(&source),
            directory.path().join("accounts.toml")
        ))
        .contains("campo display_name deve essere una stringa non vuota")
    );
}

#[test]
fn test_parse_accounts_rejects_duplicate_patient_ids() {
    let directory = TestDirectory::new();
    let mut source = accounts_text(directory.path());
    let person = source[source.find("[[person]]").unwrap()..].to_owned();
    source.push_str(&person);
    assert!(
        message(parse_accounts_data(
            &parse_value(&source),
            directory.path().join("accounts.toml")
        ))
        .contains("id paziente duplicato: patient-a")
    );
}

#[test]
fn test_parse_accounts_rejects_unknown_root_and_global_fields() {
    let directory = TestDirectory::new();
    for (source, expected) in [
        (
            format!("custom = true\n{}", accounts_text(directory.path())),
            "configurazione campi sconosciuti: custom",
        ),
        (
            accounts_text(directory.path())
                .replace("config_version = 1", "config_version = 1\ncustom = true"),
            "[global] campi sconosciuti: custom",
        ),
    ] {
        assert!(
            message(parse_accounts_data(
                &parse_value(&source),
                directory.path().join("accounts.toml")
            ))
            .contains(expected)
        );
    }
}

#[test]
fn test_parse_accounts_rejects_unknown_nested_configuration_fields() {
    let directory = TestDirectory::new();
    let cases = [
        (
            accounts_text(directory.path())
                .replace("[[person]]", "[global.search]\ncustom = true\n[[person]]"),
            "[global.search] campi sconosciuti: custom",
        ),
        (
            accounts_text(directory.path()).replace(
                "[[person]]",
                "[global.ingestion]\ncustom = true\n[[person]]",
            ),
            "[global.ingestion] campi sconosciuti: custom",
        ),
        (
            accounts_text(directory.path())
                .replace("[[person]]", "[global.usb]\ncustom = true\n[[person]]"),
            "[global.usb] campi sconosciuti: custom",
        ),
        (
            accounts_text(directory.path()).replace(
                "usb_uuid = \"1A2B-3C4D\"",
                "usb_uuid = \"1A2B-3C4D\"\n[person.search]\ncustom = true",
            ),
            "[[person]] entry 0 search campi sconosciuti: custom",
        ),
        (
            accounts_text(directory.path()).replace(
                "usb_uuid = \"1A2B-3C4D\"",
                "usb_uuid = \"1A2B-3C4D\"\n[person.ingestion]\ncustom = true",
            ),
            "[[person]] entry 0 ingestion campi sconosciuti: custom",
        ),
    ];
    for (source, expected) in cases {
        assert!(
            message(parse_accounts_data(
                &parse_value(&source),
                directory.path().join("accounts.toml")
            ))
            .contains(expected)
        );
    }
}

#[test]
fn test_load_accounts_rejects_invalid_search_ingestion_and_usb_values() {
    let directory = TestDirectory::new();
    let cases = [
        (
            "[global.search]\nadvanced_index_warning_mb = 0",
            "intero positivo",
        ),
        (
            "[global.ingestion]\nexclude_patterns = [\"\"]",
            "stringhe non vuote",
        ),
        ("[global.usb]\nmin_free_space_mb = 0", "intero positivo"),
        (
            "[global.usb]\nrequire_exfat = \"true\"",
            "deve essere booleano",
        ),
        ("[global.usb]\ncopy_strategy = \"copy-all\"", "uno tra"),
    ];
    for (section, expected) in cases {
        let source = accounts_text(directory.path())
            .replace("[[person]]", &format!("{section}\n[[person]]"));
        let path = directory
            .path()
            .join(format!("invalid-{}.toml", expected.replace(' ', "-")));
        fs::write(&path, source).expect("fixture di sezione invalida");
        assert!(message(load_accounts(path)).contains(expected), "{section}");
    }
}

#[test]
fn test_load_accounts_requires_optional_asset_paths_to_exist() {
    let directory = TestDirectory::new();
    let source = accounts_text(directory.path()).replace(
        "[[person]]",
        "[global.ui]\nbackground_image = \"missing.png\"\n[[person]]",
    );
    let path = directory.accounts_file(&source);
    assert!(message(load_accounts(path)).contains("campo background_image percorso inesistente"));
}

#[test]
fn test_load_accounts_rejects_unknown_dictionary_sections_and_invalid_entries() {
    let directory = TestDirectory::new();
    let invalid_dictionaries = [
        ("other = {}", "sezioni sconosciute: other"),
        ("[terms]\nrx = []", "lista non vuota di stringhe"),
        ("[terms]\nrx = [\"\"]", "solo stringhe non vuote"),
        ("terms = \"bad\"", ".terms deve essere una tabella"),
    ];
    for (dictionary_contents, expected) in invalid_dictionaries {
        let dictionary = directory.path().join("dictionary.toml");
        fs::write(&dictionary, dictionary_contents).expect("dizionario invalido");
        let source = format!(
            r##"
            [global]
            config_version = 1
            [global.search]
            dictionary = "{}"
            [[person]]
            id = "patient-a"
            display_name = "Patient A"
            source_documents = "{}"
            metadata_directory = "{}"
            local_build = "{}"
            usb_uuid = "1A2B-3C4D"
            "##,
            dictionary.display(),
            directory.path().join("source").display(),
            directory.path().join("metadata").display(),
            directory.path().join("generated").display(),
        );
        let path = directory.path().join("accounts.toml");
        fs::write(&path, source).expect("configurazione sintetica");
        assert!(
            message(load_accounts(path)).contains(expected),
            "{dictionary_contents}"
        );
    }
}
