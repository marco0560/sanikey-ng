use super::{DicomStudy, catalog_dicom_studies};
use crate::config::{IngestionConfig, PersonConfig, SearchConfig, UiConfig};
use crate::metadata::DocumentRecord;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sanikey-dicom-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("directory temporanea");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn person(root: &Path) -> PersonConfig {
    PersonConfig {
        id: "persona-test".to_owned(),
        display_name: "Persona di test".to_owned(),
        source_documents: root.join("source"),
        metadata_directory: root.join("metadata"),
        local_build: root.join("build"),
        usb_uuid: String::new(),
        enabled: true,
        ui: UiConfig::default(),
        search: SearchConfig::default(),
        ingestion: IngestionConfig::default(),
    }
}

fn document(path: PathBuf, kind: &str, id: &str, container_id: Option<&str>) -> DocumentRecord {
    DocumentRecord {
        document_id: id.to_owned(),
        patient_id: "persona-test".to_owned(),
        path,
        title: "DICOM sintetico".to_owned(),
        category: "uncategorized".to_owned(),
        kind: kind.to_owned(),
        sha256: id.to_owned(),
        date: None,
        series: None,
        tags: Vec::new(),
        origin: if container_id.is_some() {
            "container"
        } else {
            "source"
        }
        .to_owned(),
        container_id: container_id.map(str::to_owned),
        internal_path: None,
    }
}

fn dicom_file(path: &Path, uid: Option<&str>, date: Option<&str>, description: Option<&str>) {
    let mut dataset = Vec::new();
    if let Some(date) = date {
        element(&mut dataset, (0x0008, 0x0020), "DA", date.as_bytes());
    }
    if let Some(description) = description {
        element(&mut dataset, (0x0008, 0x1030), "LO", description.as_bytes());
    }
    if let Some(uid) = uid {
        element(&mut dataset, (0x0020, 0x000d), "UI", uid.as_bytes());
    }
    write_dicom(path, dataset);
}

fn dicomdir(path: &Path, records: &[(&str, &str, &str)]) {
    let mut sequence = Vec::new();
    for (uid, date, description) in records {
        let mut item = Vec::new();
        element(&mut item, (0x0004, 0x1430), "CS", b"STUDY");
        element(&mut item, (0x0008, 0x0020), "DA", date.as_bytes());
        element(&mut item, (0x0008, 0x1030), "LO", description.as_bytes());
        element(&mut item, (0x0020, 0x000d), "UI", uid.as_bytes());
        sequence.extend_from_slice(&[0xfe, 0xff, 0x00, 0xe0]);
        sequence.extend_from_slice(&(item.len() as u32).to_le_bytes());
        sequence.extend_from_slice(&item);
    }
    let mut dataset = Vec::new();
    dataset.extend_from_slice(&[0x04, 0x00, 0x20, 0x12]);
    dataset.extend_from_slice(b"SQ\0\0");
    dataset.extend_from_slice(&(sequence.len() as u32).to_le_bytes());
    dataset.extend_from_slice(&sequence);
    write_dicom(path, dataset);
}

fn write_dicom(path: &Path, dataset: Vec<u8>) {
    let mut meta = Vec::new();
    element(&mut meta, (0x0002, 0x0001), "OB", &[0, 1]);
    element(&mut meta, (0x0002, 0x0002), "UI", b"1.2.840.10008.1.3.10");
    element(
        &mut meta,
        (0x0002, 0x0003),
        "UI",
        b"1.2.826.0.1.3680043.10.999.1",
    );
    element(&mut meta, (0x0002, 0x0010), "UI", b"1.2.840.10008.1.2.1");
    element(
        &mut meta,
        (0x0002, 0x0012),
        "UI",
        b"1.2.826.0.1.3680043.10.999",
    );
    let mut bytes = vec![0; 128];
    bytes.extend_from_slice(b"DICM");
    element(
        &mut bytes,
        (0x0002, 0x0000),
        "UL",
        &(meta.len() as u32).to_le_bytes(),
    );
    bytes.extend_from_slice(&meta);
    bytes.extend_from_slice(&dataset);
    fs::write(path, bytes).expect("scrittura DICOM sintetico");
}

fn element(output: &mut Vec<u8>, tag: (u16, u16), vr: &str, value: &[u8]) {
    let mut value = value.to_vec();
    if !value.len().is_multiple_of(2) {
        value.push(if vr == "UI" { 0 } else { b' ' });
    }
    output.extend_from_slice(&tag.0.to_le_bytes());
    output.extend_from_slice(&tag.1.to_le_bytes());
    output.extend_from_slice(vr.as_bytes());
    if matches!(vr, "OB" | "OW" | "SQ" | "UN" | "UT") {
        output.extend_from_slice(&[0, 0]);
        output.extend_from_slice(&(value.len() as u32).to_le_bytes());
    } else {
        output.extend_from_slice(&(value.len() as u16).to_le_bytes());
    }
    output.extend_from_slice(&value);
}

fn studies(root: &Path, documents: &[DocumentRecord]) -> Vec<DicomStudy> {
    catalog_dicom_studies(&person(root), documents)
}

#[test]
fn catalog_groups_instances_by_uid_and_keeps_sorted_provenance() {
    let temp = TempDir::new();
    let one = temp.path().join("z.dcm");
    let two = temp.path().join("a.dcm");
    dicom_file(&one, Some("1.2.3"), Some("20260102"), Some("Studio"));
    dicom_file(&two, Some("1.2.3"), Some("20260102"), Some("Studio"));

    let result = studies(
        temp.path(),
        &[
            document(one, "dicom_file", "id-z", None),
            document(two.clone(), "dicom_file", "id-a", None),
        ],
    );

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].support_kind, "dicom_study");
    assert_eq!(result[0].study_instance_uid.as_deref(), Some("1.2.3"));
    assert_eq!(result[0].study_date.as_deref(), Some("2026-01-02"));
    assert_eq!(result[0].study_description.as_deref(), Some("Studio"));
    assert_eq!(result[0].instance_count, 2);
    assert_eq!(
        result[0].support_paths,
        vec![two, temp.path().join("z.dcm")]
    );
}

#[test]
fn catalog_keeps_distinct_and_uid_less_instances_as_separate_records() {
    let temp = TempDir::new();
    let first = temp.path().join("first.dcm");
    let second = temp.path().join("second.dcm");
    dicom_file(&first, Some("1.2.3"), None, None);
    dicom_file(&second, None, None, None);

    let result = studies(
        temp.path(),
        &[
            document(first, "dicom_file", "id-1", None),
            document(second, "dicom_file", "id-2", None),
        ],
    );

    assert_eq!(result.len(), 2);
    assert!(
        result
            .iter()
            .any(|study| study.study_instance_uid.as_deref() == Some("1.2.3"))
    );
    assert!(
        result
            .iter()
            .any(|study| study.warnings.iter().any(|warning| warning.contains("UID")))
    );
}

#[test]
fn catalog_reads_dicomdir_and_prefers_instances_when_coalescing_uid() {
    let temp = TempDir::new();
    let directory = temp.path().join("DICOMDIR");
    let instance = temp.path().join("instance.dcm");
    dicomdir(
        &directory,
        &[
            ("1.2.3", "20260102", "Dal DICOMDIR"),
            ("1.2.3", "20260102", "Duplicato"),
            ("1.2.4", "20260103", "Secondo studio"),
        ],
    );
    dicom_file(
        &instance,
        Some("1.2.3"),
        Some("20260102"),
        Some("Dall'istanza"),
    );

    let result = studies(
        temp.path(),
        &[
            document(directory, "binary", "id-dir", None),
            document(instance.clone(), "dicom_file", "id-instance", None),
        ],
    );

    assert_eq!(result.len(), 2);
    let merged = result
        .iter()
        .find(|study| study.study_instance_uid.as_deref() == Some("1.2.3"))
        .unwrap();
    assert_eq!(merged.support_kind, "dicom_study");
    assert_eq!(merged.instance_count, 1);
    assert_eq!(merged.support_paths, vec![instance]);
    assert!(
        result
            .iter()
            .any(|study| study.study_instance_uid.as_deref() == Some("1.2.4"))
    );
}

#[test]
fn catalog_preserves_staged_container_provenance() {
    let temp = TempDir::new();
    let container = document(
        temp.path().join("studio.iso"),
        "dicom_iso",
        "container-1",
        None,
    );
    let stage = temp.path().join("build/staging/containers/container-1");
    let instance_path = stage.join("IM000001.dcm");
    fs::create_dir_all(instance_path.parent().unwrap()).unwrap();
    dicom_file(&instance_path, Some("1.2.3"), None, None);
    let instance = document(
        instance_path,
        "dicom_file",
        "instance-1",
        Some("container-1"),
    );

    let result = studies(temp.path(), &[container.clone(), instance]);
    let grouped = result
        .iter()
        .find(|study| study.support_kind == "dicom_study")
        .unwrap();

    assert_eq!(grouped.support_path, container.path);
    assert_eq!(grouped.extracted_path.as_deref(), Some(stage.as_path()));
    assert_eq!(grouped.instance_count, 1);
    assert!(
        result
            .iter()
            .find(|study| study.study_id == "container-1")
            .unwrap()
            .warnings
            .is_empty()
    );
}

#[test]
fn catalog_reports_damaged_dicom_without_discarding_its_provenance() {
    let temp = TempDir::new();
    let path = temp.path().join("damaged.dcm");
    fs::write(&path, b"fixture DICOM danneggiato sintetico").unwrap();

    let result = studies(
        temp.path(),
        &[document(path.clone(), "dicom_file", "damaged-id", None)],
    );

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].study_id, "damaged-id");
    assert_eq!(result[0].support_path, path);
    assert_eq!(result[0].instance_count, 1);
    assert!(!result[0].warnings.is_empty());
}
