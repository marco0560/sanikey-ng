//! Verifiche sintetiche per importare ed esportare profili account.

use super::AccountsProfile;
use crate::config::tests::{CURRENT_DIRECTORY_LOCK, TestDirectory, WorkingDirectory};
use std::fs;

#[test]
fn profile_copy_survives_working_directory_change_and_reopens_with_unicode_paths() {
    let directory = TestDirectory::new();
    let root = directory.path().join("archivio clinico è");
    let config_directory = root.join("config");
    fs::create_dir_all(&config_directory).expect("directory config");
    for relative in [
        "local-data/Paziente è/documenti originali",
        "local-data/Paziente è/metadati curati",
        "local-data/artefatti generati",
        "local-data/sfondi",
    ] {
        fs::create_dir_all(root.join(relative)).expect("directory profilo sintetico");
    }
    fs::write(
        root.join("local-data/sfondi/tema è.png"),
        b"fixture sintetica",
    )
    .expect("sfondo sintetico");
    fs::write(
        config_directory.join("search-dictionary.toml"),
        "[terms]\ncefalea = [\"mal di testa\"]\n",
    )
    .expect("dizionario sintetico");

    let source = config_directory.join("accounts.toml");
    let original_contents = r##"
[global]
repository_version = 1

[global.ui]
background_image = "local-data/sfondi/tema è.png"

[global.search]
dictionary = "config/search-dictionary.toml"

[[person]]
id = "patient-a"
display_name = "Paziente è"
source_documents = "local-data/Paziente è/documenti originali"
metadata_directory = "local-data/Paziente è/metadati curati"
local_build = "local-data/artefatti generati"
usb_uuid = "1A2B-3C4D"
"##;
    fs::write(&source, original_contents).expect("profilo legacy sintetico");
    let profile = AccountsProfile::import(&source).expect("importazione legacy");
    let destination = config_directory.join("accounts-verificato.toml");

    let _lock = CURRENT_DIRECTORY_LOCK
        .lock()
        .expect("lock directory corrente");
    let _working_directory = WorkingDirectory::enter(directory.path());
    let exported = profile
        .export_copy(&destination)
        .expect("esportazione verificata dopo il cambio cwd");

    assert_eq!(
        profile.base_directory(),
        root.canonicalize().expect("root assoluta")
    );
    assert_eq!(
        exported,
        destination.canonicalize().expect("export assoluto")
    );
    assert_eq!(
        fs::read_to_string(&source).expect("sorgente leggibile"),
        original_contents
    );
    assert_eq!(
        fs::read_to_string(&exported).expect("copia leggibile"),
        original_contents
    );

    let reopened = AccountsProfile::import(&exported).expect("riapertura della copia");
    let mut reopened_accounts = reopened.accounts().clone();
    reopened_accounts.path = profile.accounts().path.clone();
    assert_eq!(&reopened_accounts, profile.accounts());
    assert_eq!(
        reopened.accounts().people[0].source_documents,
        root.join("local-data/Paziente è/documenti originali")
    );
    assert_eq!(
        reopened.accounts().ui.background_image.as_deref(),
        Some(root.join("local-data/sfondi/tema è.png").as_path())
    );
    assert_eq!(
        reopened.accounts().search.dictionary.as_deref(),
        Some(config_directory.join("search-dictionary.toml").as_path())
    );
}

#[test]
fn profile_export_rejects_overwrite_and_a_different_path_base() {
    let directory = TestDirectory::new();
    let config_directory = directory.path().join("config");
    fs::create_dir_all(&config_directory).expect("directory config");
    for relative in ["source", "metadata", "generated"] {
        fs::create_dir_all(directory.path().join(relative)).expect("directory profilo");
    }
    let source = config_directory.join("accounts.toml");
    fs::write(
        &source,
        "[global]\nconfig_version = 1\n\n[[person]]\nid = \"patient-a\"\ndisplay_name = \"Patient A\"\nsource_documents = \"source\"\nmetadata_directory = \"metadata\"\nlocal_build = \"generated\"\nusb_uuid = \"1A2B-3C4D\"\n",
    )
    .expect("config account sintetica");
    let profile = AccountsProfile::import(&source).expect("importazione profilo");

    assert!(AccountsProfile::import("config/accounts.toml").is_err());
    assert!(profile.export_copy(&source).is_err());
    let existing = config_directory.join("existing.toml");
    fs::write(&existing, "contenuto da conservare").expect("file destinazione");
    assert!(profile.export_copy(&existing).is_err());
    assert_eq!(
        fs::read_to_string(&existing).expect("file preservato"),
        "contenuto da conservare"
    );
    let different_base = directory.path().join("different-base");
    fs::create_dir_all(&different_base).expect("base alternativa");
    assert!(
        profile
            .export_copy(different_base.join("profile.toml"))
            .is_err()
    );
    assert!(profile.export_copy("relative-profile.toml").is_err());
}
