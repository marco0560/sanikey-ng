use super::{assemble_usb_image, validate_usb_image};
use crate::build::{BuildOptions, build_patient};
use crate::config::AccountsProfile;
use crate::operation::{CancellationToken, OperationOutcome};
use crate::usb_copy::mirror_validated_usb_image;
#[cfg(target_os = "linux")]
use crate::usb_copy::mirror_validated_usb_image_on_linux_volume;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sanikey-ng-usb-image-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn profile(directory: &TestDirectory) -> AccountsProfile {
    let mut people = String::new();
    for (id, name) in [("patient-a", "Persona A & B"), ("patient-b", "Persona B")] {
        let source = directory.path().join("patients").join(id).join("documents");
        let metadata = directory.path().join("patients").join(id).join("metadata");
        let local_build = directory.path().join("generated").join(id);
        fs::create_dir_all(&source).expect("documenti sintetici");
        fs::create_dir_all(&metadata).expect("metadati sintetici");
        fs::write(
            source.join("report.txt"),
            format!("Contenuto sintetico {id}"),
        )
        .expect("documento sintetico");
        people.push_str(&format!(
            "[[person]]\nid = \"{id}\"\ndisplay_name = \"{name}\"\nsource_documents = \"{}\"\nmetadata_directory = \"{}\"\nlocal_build = \"{}\"\nusb_uuid = \"1A2B-3C4D\"\nenabled = true\n",
            source.display(), metadata.display(), local_build.display()
        ));
    }
    let config_directory = directory.path().join("config");
    fs::create_dir_all(&config_directory).expect("directory config sintetica");
    let profile_path = config_directory.join("accounts.toml");
    fs::write(
        profile_path,
        format!("[global]\nconfig_version = 1\n{people}"),
    )
    .expect("profilo TOML sintetico");
    AccountsProfile::import(config_directory.join("accounts.toml"))
        .expect("profilo sintetico valido")
}

fn build_people(profile: &AccountsProfile) -> Vec<crate::build::PatientBuildResult> {
    profile
        .accounts()
        .enabled_people()
        .map(|person| build_patient(person, BuildOptions::default()).expect("build sintetica"))
        .collect()
}

#[test]
fn assembles_and_validates_a_multi_person_usb_image() {
    let directory = TestDirectory::new();
    let profile = profile(&directory);
    let builds = build_people(&profile);
    let target = directory.path().join("exports/usb-image");

    let result = assemble_usb_image(profile.accounts(), &builds, &target)
        .expect("immagine multi-persona assemblata");

    assert_eq!(result.patients, 2);
    assert!(result.files > 5);
    assert!(result.bytes > 0);
    assert!(result.manifest.is_file());
    assert!(
        target
            .join("patients/patient-a/medical_archive.db")
            .is_file()
    );
    assert!(
        target
            .join("patients/patient-b/medical_archive.db")
            .is_file()
    );
    assert!(
        target
            .join("patients/patient-a/documents/report.txt")
            .is_file()
    );
    let index = fs::read_to_string(target.join("index.html")).expect("indice USB");
    assert!(index.contains("Persona A &amp; B"));
    validate_usb_image(&target).expect("immagine USB valida");

    let unlisted = target.join("patients/patient-a/unlisted.txt");
    fs::write(&unlisted, "contenuto aggiunto senza checksum").expect("file extra sintetico");
    assert!(validate_usb_image(&target).is_err());
    fs::remove_file(unlisted).expect("rimozione file extra sintetico");

    let physical_volume = directory.path().join("synthetic-usb");
    fs::create_dir(&physical_volume).expect("target sintetico esistente");
    let copy = mirror_validated_usb_image(&target, &physical_volume, &CancellationToken::default());
    assert!(matches!(copy.outcome, OperationOutcome::Succeeded(_)));
    validate_usb_image(&physical_volume).expect("copia USB multi-persona valida");

    #[cfg(target_os = "linux")]
    {
        let linux_volume = directory.path().join("synthetic-linux-volume");
        fs::create_dir(&linux_volume).expect("volume Linux sintetico");
        let copy = mirror_validated_usb_image_on_linux_volume(
            profile.accounts(),
            &target,
            &linux_volume,
            &CancellationToken::default(),
        );
        assert!(matches!(copy.outcome, OperationOutcome::Succeeded(_)));
        validate_usb_image(linux_volume.join("SaniKey"))
            .expect("immagine validata nella cartella applicativa del volume");
    }

    fs::write(
        target.join("patients/patient-b/documents/report.txt"),
        "modifica sintetica",
    )
    .expect("alterazione sintetica");
    assert!(validate_usb_image(&target).is_err());
}

#[test]
fn rejects_existing_targets_and_builds_missing_an_enabled_person() {
    let directory = TestDirectory::new();
    let profile = profile(&directory);
    let builds = build_people(&profile);
    let existing = directory.path().join("existing-image");
    fs::create_dir(&existing).expect("target esistente");
    assert!(assemble_usb_image(profile.accounts(), &builds, &existing).is_err());
    assert!(
        assemble_usb_image(
            profile.accounts(),
            &builds[..1],
            directory.path().join("missing-build")
        )
        .is_err()
    );
    assert!(
        assemble_usb_image(
            profile.accounts(),
            &[builds[0].clone(), builds[0].clone()],
            directory.path().join("duplicate-builds")
        )
        .is_err()
    );
}
