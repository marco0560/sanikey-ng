//! Config tests for loading.

use super::*;

#[test]
fn test_load_accounts_accepts_valid_synthetic_config() {
    let directory = TestDirectory::new();
    let path = directory.accounts_file(&accounts_text(directory.path()));

    let config = load_accounts(&path).expect("configurazione valida");

    assert_eq!(config.config_version, 1);
    assert_eq!(config.people[0].id, "patient-a");
    assert_eq!(
        config.enabled_people().next().unwrap().display_name,
        "Patient A"
    );
    assert_eq!(config.ui.default_tab, "documents");
    assert_eq!(config.ui.accent_color, "#2563eb");
    assert_eq!(config.ui.density, "comfortable");
    assert_eq!(config.people[0].ui.timeline_order, "desc");
    assert_eq!(config.search.advanced_index_warning_mb, 25);
    assert_eq!(config.usb.min_free_space_mb, 256);
    assert_eq!(config.usb.copy_strategy, "rsync-preferred");
    assert!(!config.usb.require_exfat);
    assert!(config.ingestion.exclude_patterns.is_empty());
}

#[test]
fn test_load_accounts_parses_ui_defaults_and_person_override() {
    let directory = TestDirectory::new();
    let root = directory.path();
    let path = directory.accounts_file(&format!(
        r##"
        [global]
        config_version = 1
        [global.ui]
        accent_color = "#1D4ED8"
        density = "compact"
        default_tab = "timeline"
        timeline_order = "asc"
        document_link_mode = "usb-relative"
        subtitle = "Archivio condiviso"
        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "{}"
        metadata_directory = "{}"
        local_build = "{}"
        usb_uuid = "1A2B-3C4D"
        [person.ui]
        default_tab = "documents"
        subtitle = "Archivio Patient A"
        "##,
        root.join("source").display(),
        root.join("metadata").display(),
        root.join("generated").display(),
    ));

    let config = load_accounts(path).expect("override UI valido");

    assert_eq!(config.ui.default_tab, "timeline");
    assert_eq!(config.ui.accent_color, "#1d4ed8");
    assert_eq!(config.people[0].ui.density, "compact");
    assert_eq!(config.people[0].ui.timeline_order, "asc");
    assert_eq!(config.people[0].ui.default_tab, "documents");
    assert_eq!(config.people[0].ui.subtitle, "Archivio Patient A");
}

#[test]
fn test_load_accounts_parses_search_and_background_config() {
    let directory = TestDirectory::new();
    let dictionary = directory.path().join("dictionary.toml");
    fs::write(
        &dictionary,
        "[terms]\nrx = [\"radiografia\", \"raggi x\"]\nek = [\"elettrocardiogramma\"]\n[months]\ngennaio = [\"01\", \"1\"]\n",
    )
    .expect("dizionario sintetico");
    let background = directory.path().join("background.png");
    fs::write(&background, b"synthetic").expect("immagine sintetica");
    let path = directory.accounts_file(&format!(
        r##"
        [global]
        config_version = 1
        [global.search]
        dictionary = "{}"
        advanced_index_warning_mb = 2
        [global.ui]
        background_image = "{}"
        background_opacity = 0.2
        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "{}"
        metadata_directory = "{}"
        local_build = "{}"
        usb_uuid = "1A2B-3C4D"
        "##,
        dictionary.display(),
        background.display(),
        directory.path().join("source").display(),
        directory.path().join("metadata").display(),
        directory.path().join("generated").display(),
    ));

    let config = load_accounts(path).expect("ricerca e sfondo validi");

    assert_eq!(
        config.search.dictionary.as_deref(),
        Some(dictionary.as_path())
    );
    assert_eq!(
        config.search.dictionary_data.terms["rx"],
        ["radiografia", "raggi x"]
    );
    assert_eq!(
        config
            .search
            .dictionary_data
            .terms
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["rx", "ek"]
    );
    assert_eq!(config.search.dictionary_data.months["gennaio"], ["01", "1"]);
    assert_eq!(config.people[0].search.advanced_index_warning_mb, 2);
    assert_eq!(
        config.people[0].ui.background_image.as_deref(),
        Some(background.as_path())
    );
    assert_eq!(config.people[0].ui.background_opacity, 0.2);
}

#[test]
fn test_load_accounts_person_search_override_replaces_dictionary_and_threshold() {
    let directory = TestDirectory::new();
    let global_dictionary = directory.path().join("global-dictionary.toml");
    let person_dictionary = directory.path().join("person-dictionary.toml");
    fs::write(&global_dictionary, "[terms]\nrx = [\"radiografia\"]\n").expect("dizionario globale");
    fs::write(
        &person_dictionary,
        "[terms]\necg = [\"elettrocardiogramma\"]\n",
    )
    .expect("dizionario persona");
    let path = directory.accounts_file(&format!(
        r##"
        [global]
        config_version = 1
        [global.search]
        dictionary = "{}"
        advanced_index_warning_mb = 20
        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "{}"
        metadata_directory = "{}"
        local_build = "{}"
        usb_uuid = "1A2B-3C4D"
        [person.search]
        dictionary = "{}"
        advanced_index_warning_mb = 5
        "##,
        global_dictionary.display(),
        directory.path().join("source").display(),
        directory.path().join("metadata").display(),
        directory.path().join("generated").display(),
        person_dictionary.display(),
    ));

    let config = load_accounts(path).expect("override ricerca valido");

    assert_eq!(config.search.advanced_index_warning_mb, 20);
    assert!(config.search.dictionary_data.terms.contains_key("rx"));
    assert_eq!(config.people[0].search.advanced_index_warning_mb, 5);
    assert!(
        config.people[0]
            .search
            .dictionary_data
            .terms
            .contains_key("ecg")
    );
    assert!(
        !config.people[0]
            .search
            .dictionary_data
            .terms
            .contains_key("rx")
    );
}

#[test]
fn test_load_accounts_parses_ingestion_and_usb_config() {
    let directory = TestDirectory::new();
    let root = directory.path();
    let path = directory.accounts_file(&format!(
        r##"
        [global]
        config_version = 1
        [global.ingestion]
        exclude_patterns = ["**/Help/**", "*.tmp"]
        include_patterns = ["**/Help/keep.txt"]
        [global.usb]
        usb_uuid = "1A2B-3C4D"
        require_exfat = true
        min_free_space_mb = 512
        copy_strategy = "python"
        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "{}"
        metadata_directory = "{}"
        local_build = "{}"
        usb_uuid = "1A2B-3C4D"
        [person.ingestion]
        exclude_patterns = ["**/Viewer-Windows/**"]
        include_patterns = ["**/Viewer-Windows/report.txt"]
        "##,
        root.join("source").display(),
        root.join("metadata").display(),
        root.join("generated").display(),
    ));

    let config = load_accounts(path).expect("ingestione e USB valide");

    assert_eq!(config.ingestion.exclude_patterns, ["**/Help/**", "*.tmp"]);
    assert_eq!(config.ingestion.include_patterns, ["**/Help/keep.txt"]);
    assert_eq!(config.usb.usb_uuid.as_deref(), Some("1A2B-3C4D"));
    assert!(config.usb.require_exfat);
    assert_eq!(config.usb.min_free_space_mb, 512);
    assert_eq!(config.usb.copy_strategy, "python");
    assert_eq!(
        config.people[0].ingestion.exclude_patterns,
        ["**/Help/**", "*.tmp", "**/Viewer-Windows/**"]
    );
    assert_eq!(
        config.people[0].ingestion.include_patterns,
        ["**/Help/keep.txt", "**/Viewer-Windows/report.txt"]
    );
}

#[test]
fn test_load_accounts_rejects_unknown_person_field() {
    let directory = TestDirectory::new();
    let source = accounts_text(directory.path()).replace(
        "usb_uuid = \"1A2B-3C4D\"",
        "usb_uuid = \"1A2B-3C4D\"\nexclude_patterns = [\"**/Help/**\"]",
    );
    let path = directory.accounts_file(&source);

    assert!(
        message(load_accounts(path))
            .contains("voce [[person]] 0 campi sconosciuti: exclude_patterns")
    );
}
