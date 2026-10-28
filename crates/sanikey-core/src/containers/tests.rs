use super::{is_container_path, safe_member_path, stage_container_documents};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::inventory::scan_document_inventory;
use serde_json::Value;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-containers-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea");
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

fn person(root: &Path) -> PersonConfig {
    PersonConfig {
        id: "patient-a".to_owned(),
        display_name: "Patient A".to_owned(),
        source_documents: root.join("documents"),
        metadata_directory: root.join("metadata"),
        local_build: root.join("generated"),
        usb_uuid: "1A2B-3C4D".to_owned(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    zip_bytes_with_method(entries, zip::CompressionMethod::Deflated)
}

fn zip_bytes_with_method(entries: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default().compression_method(method),
            )
            .expect("membro ZIP sintetico");
        writer.write_all(bytes).expect("contenuto ZIP sintetico");
    }
    writer.finish().expect("ZIP sintetico").into_inner()
}

#[test]
fn reads_python_zipfile_compression_methods() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    for (name, method) in [
        ("deflate.zip", zip::CompressionMethod::Deflated),
        ("bzip2.zip", zip::CompressionMethod::Bzip2),
    ] {
        fs::write(
            person.source_documents.join(name),
            zip_bytes_with_method(&[("report.txt", b"Synthetic report")], method),
        )
        .expect("archivio ZIP sintetico");
    }
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/zip-lzma.zip"),
        person.source_documents.join("lzma.zip"),
    )
    .expect("fixture ZIP LZMA sintetica");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging ZIP con i metodi della baseline");

    assert_eq!(result.documents.len(), 3);
    assert!(result.warning_messages.is_empty());
    for document in result.documents {
        assert_eq!(
            fs::read(document.path).expect("contenuto ZIP"),
            b"Synthetic report"
        );
    }
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    fs::write(path, zip_bytes(entries)).expect("archivio ZIP sintetico");
}

fn source_inventory(person: &PersonConfig) -> Vec<crate::metadata::DocumentRecord> {
    scan_document_inventory(person).expect("inventario sintetico")
}

fn command_available(name: &str) -> bool {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).any(|directory| {
        [name.to_owned(), format!("{name}.exe")]
            .iter()
            .any(|candidate| directory.join(candidate).is_file())
    })
}

#[test]
fn stages_zip_members_and_writes_a_provenance_manifest() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let archive = person.source_documents.join("archive.ZIP");
    write_zip(
        &archive,
        &[
            ("reports/20260102_result.txt", b"Synthetic report"),
            ("technical/source.bin", b"ignored by the document pipeline"),
        ],
    );

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging ZIP sintetico");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(result.members.len(), 2);
    assert_eq!(
        result.documents[0].internal_path.as_deref(),
        Some("reports/20260102_result.txt")
    );
    assert_eq!(result.documents[0].origin, "container");
    assert!(
        result.documents[0]
            .path
            .starts_with(person.local_build.join("staging/containers"))
    );
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro estratto"),
        b"Synthetic report"
    );

    let manifest: Value = serde_json::from_slice(&fs::read(&result.manifest).expect("manifest"))
        .expect("JSON manifest");
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["patient_id"], "patient-a");
    assert_eq!(manifest["members"].as_array().expect("membri").len(), 2);
    assert!(manifest["warnings"].as_array().expect("warning").is_empty());
}

#[test]
fn writes_an_empty_manifest_when_the_inventory_has_no_containers() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(
        person.source_documents.join("report.txt"),
        b"Synthetic report",
    )
    .expect("documento sintetico");

    let result =
        stage_container_documents(&person, &source_inventory(&person)).expect("manifest vuoto");

    assert!(result.documents.is_empty());
    assert!(result.members.is_empty());
    assert!(result.warning_messages.is_empty());
    let manifest: Value = serde_json::from_slice(&fs::read(result.manifest).expect("manifest"))
        .expect("JSON manifest");
    assert!(manifest["members"].as_array().expect("membri").is_empty());
}

#[test]
fn rejects_zip_traversal_without_writing_outside_the_staging_root() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    write_zip(
        &person.source_documents.join("unsafe.zip"),
        &[("../../escape.txt", b"must not be written")],
    );

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("errore del singolo archivio come warning");

    assert!(result.documents.is_empty());
    assert!(result.members.is_empty());
    assert_eq!(result.warning_messages.len(), 1);
    assert!(!directory.path().join("escape.txt").exists());
}

#[test]
fn recursively_stages_nested_archives_once_and_applies_ingestion_filters() {
    let directory = TestDirectory::new();
    let mut person = person(directory.path());
    person.ingestion.exclude_patterns = vec!["*.txt".to_owned()];
    person.ingestion.include_patterns = vec!["keep.txt".to_owned()];
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let nested = zip_bytes(&[("KEEP.TXT", b"included"), ("skip.txt", b"excluded")]);
    write_zip(
        &person.source_documents.join("outer.zip"),
        &[("nested.zip", &nested), ("outer.txt", b"excluded")],
    );

    let result =
        stage_container_documents(&person, &source_inventory(&person)).expect("staging ricorsivo");

    assert_eq!(result.nested_containers.len(), 1);
    assert_eq!(result.members.len(), 4);
    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        result.documents[0].internal_path.as_deref(),
        Some("KEEP.TXT")
    );
    assert_eq!(
        result.documents[0]
            .path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("KEEP.TXT")
    );
}

#[test]
fn skips_repeated_container_documents_by_content_identity() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    write_zip(
        &person.source_documents.join("archive.zip"),
        &[("report.txt", b"Synthetic report")],
    );
    let mut inventory = source_inventory(&person);
    inventory.push(inventory[0].clone());

    let result = stage_container_documents(&person, &inventory).expect("container deduplicato");

    assert_eq!(result.members.len(), 1);
    assert_eq!(result.documents.len(), 1);
}

#[test]
fn tar_xz_stages_regular_members_and_skips_links() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let archive_path = person.source_documents.join("records.tar.xz");
    let mut tar_bytes = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(13);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, "records/report.txt", &b"Synthetic TXT"[..])
            .expect("membro TAR sintetico");
        archive.finish().expect("TAR completo");
    }
    let midpoint = tar_bytes.len() / 2;
    let mut compressed = Vec::new();
    for chunk in [&tar_bytes[..midpoint], &tar_bytes[midpoint..]] {
        let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 6);
        encoder.write_all(chunk).expect("stream XZ sintetico");
        compressed.extend(encoder.finish().expect("stream XZ completo"));
    }
    fs::write(&archive_path, compressed).expect("archivio TAR.XZ sintetico");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging TAR.XZ sintetico");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro TAR estratto"),
        b"Synthetic TXT"
    );
}

#[test]
fn stages_7z_members_with_the_validated_extraction_callback() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let input = directory.path().join("input");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::create_dir_all(&input).expect("directory sorgente 7z");
    fs::write(input.join("report.txt"), b"Synthetic 7z report").expect("membro sintetico");
    let archive = person.source_documents.join("records.7z");
    sevenz_rust2::compress_to_path(&input, &archive).expect("archivio 7z sintetico");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging 7z sintetico");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro 7z estratto"),
        b"Synthetic 7z report"
    );
}

#[test]
fn disk_image_suffix_uses_external_listing_and_streamed_member_extraction() {
    if !command_available("7zz") && !command_available("7z") {
        return;
    }
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let input = directory.path().join("image-input");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::create_dir_all(&input).expect("directory immagine sintetica");
    fs::write(input.join("report.txt"), b"Synthetic image member").expect("membro sintetico");
    let image = person.source_documents.join("sample.iso");
    sevenz_rust2::compress_to_path(&input, &image).expect("archivio sintetico col suffisso ISO");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging del percorso immagini");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro immagine estratto"),
        b"Synthetic image member"
    );
}

#[test]
fn recursively_stages_nested_disk_image_members() {
    if !command_available("7zz") && !command_available("7z") {
        return;
    }
    let directory = TestDirectory::new();
    let person = person(directory.path());
    let nested_source = directory.path().join("nested-image");
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::create_dir_all(nested_source.join("DICOM")).expect("directory DICOM sintetica");
    let mut dicom = vec![0; 128];
    dicom.extend_from_slice(b"DICM");
    fs::write(nested_source.join("DICOM/IM000001.dcm"), dicom).expect("DICOM sintetico");
    let nested_image = directory.path().join("study-image.iso");
    sevenz_rust2::compress_to_path(&nested_source, &nested_image)
        .expect("contenitore 7z sintetico con suffisso ISO");
    write_zip(
        &person.source_documents.join("study.zip"),
        &[(
            "Study.iso",
            &fs::read(nested_image).expect("immagine sintetica"),
        )],
    );

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging annidato ZIP e immagine");

    assert_eq!(result.warning_messages, Vec::<String>::new());
    assert_eq!(result.documents.len(), 2);
    assert_eq!(
        result.documents[0].internal_path.as_deref(),
        Some("Study.iso")
    );
    assert_eq!(
        result.documents[1].internal_path.as_deref(),
        Some("DICOM/IM000001.dcm")
    );
    assert_eq!(result.documents[1].kind, "dicom_file");
}

#[test]
fn invalid_rar_is_reported_without_aborting_the_manifest() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(person.source_documents.join("broken.rar"), b"not a RAR")
        .expect("contenitore RAR corrotto sintetico");

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("warning RAR non fatale");

    assert!(result.documents.is_empty());
    assert!(result.members.is_empty());
    assert_eq!(result.warning_messages.len(), 1);
}

#[test]
fn stages_rar_members_through_stdout_without_using_archive_paths() {
    if !command_available("unrar") {
        return;
    }
    let Some(rar) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(if cfg!(windows) { "rar.exe" } else { "rar" }))
            .find(|candidate| candidate.is_file())
    }) else {
        return;
    };
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    let input = directory.path().join("rar-input.txt");
    fs::write(&input, b"Synthetic RAR report").expect("membro RAR sintetico");
    let archive = person.source_documents.join("records.rar");
    let created = std::process::Command::new(rar)
        .args(["a", "-idq"])
        .arg(&archive)
        .arg(&input)
        .output()
        .expect("creazione RAR sintetico");
    assert!(
        created.status.success(),
        "RAR sintetico: {:?}",
        created.status
    );

    let result = stage_container_documents(&person, &source_inventory(&person))
        .expect("staging RAR sintetico");

    assert_eq!(result.documents.len(), 1);
    assert_eq!(
        fs::read(&result.documents[0].path).expect("membro RAR estratto"),
        b"Synthetic RAR report"
    );
}

#[test]
fn member_paths_reject_posix_and_windows_traversal() {
    let root = Path::new("/staging/container");
    for name in [
        "/etc/passwd",
        "../../escape.txt",
        "..\\..\\escape.txt",
        "C:\\outside\\file.txt",
        "D:/outside.txt",
    ] {
        assert!(safe_member_path(root, name).is_err(), "{name}");
    }
    assert!(safe_member_path(root, "reports/file.txt").is_ok());
    if cfg!(windows) {
        for name in ["CON", "NUL.txt", "COM1.log", "report:stream", "trailing. "] {
            assert!(safe_member_path(root, name).is_err(), "{name}");
        }
    }
    assert!(is_container_path(Path::new("case.TAR.XZ")));
    assert!(is_container_path(Path::new("nested.ISO")));
    assert!(!is_container_path(Path::new("document.pdf")));
}

#[test]
fn corrupt_archive_emits_warning_and_still_writes_empty_manifest() {
    let directory = TestDirectory::new();
    let person = person(directory.path());
    fs::create_dir_all(&person.source_documents).expect("directory documenti");
    fs::write(person.source_documents.join("broken.zip"), b"not a ZIP")
        .expect("contenitore corrotto sintetico");

    let result =
        stage_container_documents(&person, &source_inventory(&person)).expect("warning non fatale");

    assert!(result.documents.is_empty());
    assert!(result.members.is_empty());
    assert_eq!(result.warning_messages.len(), 1);
    assert!(result.manifest.is_file());
}
