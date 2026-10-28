use super::{export_txt_simulation, validate_txt_simulation};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-simulated-export-{}-{id}",
            std::process::id()
        ));
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

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn person(root: &Path) -> PersonConfig {
    let corpus = repository_root().join("docs/migration/corpus/minimal/patient-a");
    PersonConfig {
        id: "patient-a".to_owned(),
        display_name: "Paziente sintetico A".to_owned(),
        source_documents: corpus.join("documents"),
        metadata_directory: corpus.join("metadata"),
        local_build: root.join("generated"),
        usb_uuid: "1A2B-3C4D".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn read_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("directory sintetica leggibile") {
            let entry = entry.expect("elemento sintetico leggibile");
            let path = entry.path();
            if entry
                .file_type()
                .expect("tipo sintetico leggibile")
                .is_dir()
            {
                pending.push(path);
            } else if entry
                .file_type()
                .expect("tipo sintetico leggibile")
                .is_file()
            {
                files.push((
                    path.strip_prefix(root)
                        .expect("percorso relativo al corpus")
                        .to_path_buf(),
                    fs::read(path).expect("byte sintetici"),
                ));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[test]
fn simulated_export_is_repeatable_detects_tampering_and_preserves_originals() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let originals_before = read_tree(&person.source_documents);
    let first = export_txt_simulation(&person, directory.path().join("usb-first"))
        .expect("prima esportazione simulata");
    let second = export_txt_simulation(&person, directory.path().join("usb-second"))
        .expect("seconda esportazione simulata");

    assert_eq!(first.patients, 1);
    assert_eq!(first.files, second.files);
    assert_eq!(
        fs::read(&first.manifest).expect("primo manifest"),
        fs::read(&second.manifest).expect("secondo manifest")
    );
    validate_txt_simulation(&first.root).expect("prima esportazione valida");
    validate_txt_simulation(&second.root).expect("seconda esportazione valida");

    let linked_document = first
        .root
        .join("patients/patient-a/documents/duplicate/20260102 Copia sintetica.txt");
    assert!(linked_document.is_file());
    let page = first.root.join("patients/patient-a/web/index.html");
    assert!(
        fs::read_to_string(page)
            .expect("pagina nel target simulato")
            .contains("../documents/duplicate/20260102%20Copia%20sintetica.txt")
    );

    fs::write(&linked_document, b"tampered synthetic copy").expect("mutazione sintetica");
    assert!(validate_txt_simulation(&first.root).is_err());
    assert_eq!(read_tree(&person.source_documents), originals_before);
}

#[test]
fn simulated_export_rejects_existing_targets_and_input_overlaps() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let existing = directory.path().join("existing");
    fs::create_dir(&existing).expect("target preesistente");
    assert!(export_txt_simulation(&person, &existing).is_err());
    assert!(export_txt_simulation(&person, &person.local_build).is_err());
}
