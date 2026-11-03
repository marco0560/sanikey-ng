use super::{mirror_directory, resolve_path};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::operation::{CancellationToken, OperationEvent, OperationOutcome, OperationPhase};
use crate::simulated_export::{export_txt_simulation, validate_txt_simulation};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sanikey-usb-copy-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(root: &Path) -> PersonConfig {
    PersonConfig {
        id: "synthetic-a".to_owned(),
        display_name: "Persona sintetica".to_owned(),
        source_documents: root.join("patients/synthetic-a/documents"),
        metadata_directory: root.join("patients/synthetic-a/metadata"),
        local_build: root.join("generated/synthetic-a"),
        usb_uuid: "SYNTHETIC-UUID".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn read_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
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
            } else {
                files.push((
                    path.strip_prefix(root)
                        .expect("percorso figlio della radice")
                        .to_path_buf(),
                    fs::read(path).expect("byte sintetici leggibili"),
                ));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[test]
fn mirrors_a_manifest_verified_image_and_reuses_unchanged_files() {
    let temp = TestDirectory::new();
    let person = person(&temp.0);
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let original = person.source_documents.join("2026-01-02 report.txt");
    fs::write(&original, "Referto sintetico uno").expect("originale sintetico");
    let first_image = temp.0.join("image-first");
    export_txt_simulation(&person, &first_image).expect("prima immagine sintetica");
    let target = temp.0.join("target");
    let cancellation = CancellationToken::default();

    let first = mirror_directory(&first_image, &target, &cancellation);
    let OperationOutcome::Succeeded(first_result) = first.outcome else {
        panic!("prima copia riuscita: {:?}", first.outcome);
    };
    assert!(first_result.copied_files > 0);
    assert_eq!(first_result.reused_files, 0);
    assert_eq!(read_tree(&first_image), read_tree(&target));
    validate_txt_simulation(&target).expect("manifest e checksum dopo la copia");
    let database = target.join("patients/synthetic-a/medical_archive.db");
    assert!(database.is_file());

    let second = mirror_directory(&first_image, &target, &cancellation);
    let OperationOutcome::Succeeded(second_result) = second.outcome else {
        panic!("seconda copia riuscita: {:?}", second.outcome);
    };
    assert_eq!(second_result.copied_files, 0);
    assert_eq!(second_result.reused_files, first_result.copied_files);
    assert!(database.is_file());
    validate_txt_simulation(&target).expect("manifest stabile dopo il riuso");

    fs::write(target.join("obsolete.txt"), "file obsoleto sintetico")
        .expect("file obsoleto nel target");
    let cleaned = mirror_directory(&first_image, &target, &cancellation);
    let OperationOutcome::Succeeded(cleaned_result) = cleaned.outcome else {
        panic!("rimozione file obsoleto riuscita: {:?}", cleaned.outcome);
    };
    assert_eq!(cleaned_result.removed_files, 1);
    validate_txt_simulation(&target).expect("manifest senza file obsoleto");

    fs::remove_file(&original).expect("rimozione del vecchio sorgente sintetico");
    fs::write(
        person.source_documents.join("2026-01-03 report.txt"),
        "Referto sintetico due",
    )
    .expect("nuovo originale sintetico");
    let second_image = temp.0.join("image-second");
    export_txt_simulation(&person, &second_image).expect("seconda immagine sintetica");
    let updated = mirror_directory(&second_image, &target, &cancellation);
    let OperationOutcome::Succeeded(updated_result) = updated.outcome else {
        panic!("aggiornamento riuscito: {:?}", updated.outcome);
    };
    assert!(updated_result.copied_files > 0);
    assert_eq!(read_tree(&second_image), read_tree(&target));
    validate_txt_simulation(&target).expect("manifest e checksum aggiornati");
    assert_eq!(
        fs::read(person.source_documents.join("2026-01-03 report.txt")).unwrap(),
        b"Referto sintetico due"
    );
}

#[test]
fn rejects_unsafe_targets_before_modifying_them_and_recovers_after_correction() {
    let temp = TestDirectory::new();
    let source = temp.0.join("image");
    fs::create_dir_all(source.join("nested")).expect("directory sorgente");
    fs::write(source.join("nested/file.txt"), "byte sorgente sintetici")
        .expect("file sorgente sintetico");
    let target = temp.0.join("target");
    fs::create_dir(&target).expect("directory destinazione");
    fs::write(target.join("nested"), "preservare prima della validazione")
        .expect("conflitto sintetico");
    let source_before = read_tree(&source);
    let target_before = fs::read(target.join("nested")).expect("destinazione prima del tentativo");

    let failed = mirror_directory(&source, &target, &CancellationToken::default());
    assert!(matches!(failed.outcome, OperationOutcome::Failed(_)));
    assert_eq!(read_tree(&source), source_before);
    assert_eq!(fs::read(target.join("nested")).unwrap(), target_before);

    fs::remove_file(target.join("nested")).expect("rimozione del conflitto sintetico");
    fs::create_dir(target.join("nested")).expect("directory destinazione corretta");
    let resumed = mirror_directory(&source, &target, &CancellationToken::default());
    assert!(matches!(resumed.outcome, OperationOutcome::Succeeded(_)));
    assert_eq!(read_tree(&source), read_tree(&target));
    assert_eq!(read_tree(&source), source_before);
}

#[test]
fn reports_progress_and_honors_cancellation_before_copying() {
    let temp = TestDirectory::new();
    let source = temp.0.join("image");
    fs::create_dir(&source).expect("directory sorgente");
    fs::write(source.join("one.txt"), "contenuto sintetico").expect("file sorgente");
    let target = temp.0.join("target");
    let run = mirror_directory(&source, &target, &CancellationToken::default());
    assert!(matches!(run.outcome, OperationOutcome::Succeeded(_)));
    assert!(run.events.contains(&OperationEvent::Progress {
        phase: OperationPhase::Processing,
        completed: 1,
        total: Some(1),
    }));

    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let cancelled_target = temp.0.join("cancelled-target");
    let cancelled = mirror_directory(&source, &cancelled_target, &cancellation);
    assert_eq!(cancelled.outcome, OperationOutcome::Cancelled);
    assert!(!cancelled_target.exists());
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_in_source_and_destination() {
    use std::os::unix::fs::symlink;

    let temp = TestDirectory::new();
    let source = temp.0.join("image");
    fs::create_dir(&source).expect("directory immagine");
    let outside = temp.0.join("outside.txt");
    fs::write(&outside, "sorgente esterna").expect("file fuori immagine");
    symlink(&outside, source.join("link.txt")).expect("link sorgente sintetico");
    let target = temp.0.join("target");
    let result = mirror_directory(&source, &target, &CancellationToken::default());
    assert!(matches!(result.outcome, OperationOutcome::Failed(_)));
    assert!(!target.exists());

    fs::remove_file(source.join("link.txt")).expect("rimozione link sorgente");
    fs::create_dir(&target).expect("directory destinazione");
    let external_directory = temp.0.join("outside");
    fs::create_dir(&external_directory).expect("directory esterna");
    let target_link = temp.0.join("target-link");
    symlink(&external_directory, &target_link).expect("link destinazione sintetico");
    let result = mirror_directory(&source, &target_link, &CancellationToken::default());
    assert!(matches!(result.outcome, OperationOutcome::Failed(_)));
    assert!(read_tree(&external_directory).is_empty());
}

#[test]
fn rejects_source_target_overlap() {
    let temp = TestDirectory::new();
    let source = temp.0.join("image");
    fs::create_dir_all(source.join("nested")).expect("directory sorgente");
    let target_inside = source.join("target");
    let inside = mirror_directory(&source, &target_inside, &CancellationToken::default());
    assert!(matches!(inside.outcome, OperationOutcome::Failed(_)));
    assert!(!target_inside.exists());

    let target_parent = resolve_path(&source)
        .expect("sorgente risolta")
        .parent()
        .unwrap()
        .to_path_buf();
    let parent = mirror_directory(&source, &target_parent, &CancellationToken::default());
    assert!(matches!(parent.outcome, OperationOutcome::Failed(_)));
}
