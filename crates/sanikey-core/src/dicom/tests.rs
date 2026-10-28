use super::{DicomStudy, catalog_dicom_studies, generate_dicom_previews, prepare_dicom_media};
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

fn dicom_image(path: &Path) {
    let mut dataset = Vec::new();
    element(
        &mut dataset,
        (0x0008, 0x0016),
        "UI",
        b"1.2.840.10008.5.1.4.1.1.2",
    );
    element(
        &mut dataset,
        (0x0008, 0x0018),
        "UI",
        b"1.2.826.0.1.3680043.10.999.2",
    );
    element(&mut dataset, (0x0008, 0x0020), "DA", b"20260102");
    element(&mut dataset, (0x0008, 0x0060), "CS", b"CT");
    element(&mut dataset, (0x0010, 0x0010), "PN", b"SYNTHETIC^PATIENT");
    element(&mut dataset, (0x0010, 0x0020), "LO", b"synthetic-patient");
    element(
        &mut dataset,
        (0x0020, 0x000d),
        "UI",
        b"1.2.826.0.1.3680043.10.999.3",
    );
    element(
        &mut dataset,
        (0x0020, 0x000e),
        "UI",
        b"1.2.826.0.1.3680043.10.999.4",
    );
    element(&mut dataset, (0x0020, 0x0011), "IS", b"1");
    element(&mut dataset, (0x0020, 0x0013), "IS", b"1");
    element(&mut dataset, (0x0028, 0x0002), "US", &1_u16.to_le_bytes());
    element(&mut dataset, (0x0028, 0x0004), "CS", b"MONOCHROME2");
    element(&mut dataset, (0x0028, 0x0010), "US", &2_u16.to_le_bytes());
    element(&mut dataset, (0x0028, 0x0011), "US", &2_u16.to_le_bytes());
    element(&mut dataset, (0x0028, 0x0100), "US", &8_u16.to_le_bytes());
    element(&mut dataset, (0x0028, 0x0101), "US", &8_u16.to_le_bytes());
    element(&mut dataset, (0x0028, 0x0102), "US", &7_u16.to_le_bytes());
    element(&mut dataset, (0x0028, 0x0103), "US", &0_u16.to_le_bytes());
    element(&mut dataset, (0x7fe0, 0x0010), "OW", &[0, 64, 128, 255]);
    write_dicom_with_meta(
        path,
        dataset,
        "1.2.840.10008.5.1.4.1.1.2",
        "1.2.826.0.1.3680043.10.999.2",
    );
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
    write_dicom_with_meta(
        path,
        dataset,
        "1.2.840.10008.1.3.10",
        "1.2.826.0.1.3680043.10.999.1",
    );
}

fn write_dicom_with_meta(path: &Path, dataset: Vec<u8>, sop_class: &str, sop_instance: &str) {
    let mut meta = Vec::new();
    element(&mut meta, (0x0002, 0x0001), "OB", &[0, 1]);
    element(&mut meta, (0x0002, 0x0002), "UI", sop_class.as_bytes());
    element(&mut meta, (0x0002, 0x0003), "UI", sop_instance.as_bytes());
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

#[test]
fn media_copies_original_instances_and_a_resolvable_dicomdir() {
    let temp = TempDir::new();
    let support = document(
        temp.path().join("source/studio.iso"),
        "dicom_iso",
        "container-2",
        None,
    );
    let stage = temp.path().join("build/staging/containers/container-2");
    fs::create_dir_all(&stage).unwrap();
    let instance_path = stage.join("PT000000/ST000000/SE000000/IM000000");
    fs::create_dir_all(instance_path.parent().unwrap()).unwrap();
    dicom_image(&instance_path);
    let original_bytes = fs::read(&instance_path).unwrap();
    let directory_bytes = include_bytes!("fixtures/DICOMDIR");
    fs::write(stage.join("DICOMDIR"), directory_bytes).unwrap();
    let instance = document(
        instance_path.clone(),
        "dicom_file",
        "instance-2",
        Some("container-2"),
    );
    let directory = document(
        stage.join("DICOMDIR"),
        "binary",
        "dicomdir-2",
        Some("container-2"),
    );
    let person = person(temp.path());
    let studies = catalog_dicom_studies(&person, &[support, instance, directory]);

    let media = prepare_dicom_media(&person, &studies).unwrap();

    assert_eq!(media.len(), 1);
    assert_eq!(media[0].instance_count, 1);
    assert!(media[0].directory.join("DICOMDIR").is_file());
    assert_eq!(
        fs::read(media[0].directory.join("DICOMDIR")).unwrap(),
        directory_bytes
    );
    assert_eq!(
        fs::read(
            media[0]
                .directory
                .join("PT000000/ST000000/SE000000/IM000000")
        )
        .unwrap(),
        original_bytes
    );
    assert!(media[0].warnings.is_empty());
    assert!(
        person
            .local_build
            .join("manifests/dicom_media.json")
            .is_file()
    );
}

#[test]
fn media_keeps_relative_html_assets_and_excludes_native_launchers() {
    let temp = TempDir::new();
    let support = document(
        temp.path().join("source/studio.iso"),
        "dicom_iso",
        "container-3",
        None,
    );
    let stage = temp.path().join("build/staging/containers/container-3");
    let viewer = stage.join("IHE_PDI/PAGES/STUDIES/STUDY1.HTM");
    fs::create_dir_all(viewer.parent().unwrap()).unwrap();
    fs::write(&viewer, "<html><img src='../image.jpg'></html>").unwrap();
    fs::write(stage.join("IHE_PDI/PAGES/image.jpg"), [0xff, 0xd8, 0xff]).unwrap();
    fs::write(stage.join("viewer.exe"), b"synthetic executable marker").unwrap();
    let instance_path = stage.join("IM000001.dcm");
    dicom_image(&instance_path);
    let instance = document(
        instance_path,
        "dicom_file",
        "instance-3",
        Some("container-3"),
    );
    let person = person(temp.path());
    let studies = catalog_dicom_studies(&person, &[support, instance]);
    let catalogued = studies
        .iter()
        .find(|study| study.support_kind == "dicom_iso")
        .unwrap();

    assert_eq!(
        catalogued.html_viewer_path.as_deref(),
        Some(viewer.as_path())
    );
    let media = prepare_dicom_media(&person, &studies).unwrap();
    let copied_viewer = media[0].directory.join("IHE_PDI/PAGES/STUDIES/STUDY1.HTM");
    assert!(copied_viewer.is_file());
    assert!(media[0].directory.join("IHE_PDI/PAGES/image.jpg").is_file());
    assert!(!media[0].directory.join("viewer.exe").exists());
    assert!(!media[0].directory.join("DICOMDIR").exists());
    assert!(
        media[0]
            .warnings
            .iter()
            .any(|warning| warning.contains("DICOMDIR"))
    );
    assert!(
        !fs::read_to_string(copied_viewer)
            .unwrap()
            .contains("https://")
    );
}

#[test]
fn catalog_prefers_offline_ihe_pages_and_applies_path_exclusions() {
    let temp = TempDir::new();
    let support = document(
        temp.path().join("source/studio.iso"),
        "dicom_iso",
        "container-4",
        None,
    );
    let stage = temp.path().join("build/staging/containers/container-4");
    let preferred = stage.join("IHE_PDI/PAGES/STUDIES/STUDY1.HTM");
    fs::create_dir_all(preferred.parent().unwrap()).unwrap();
    fs::write(&preferred, "<html>offline viewer</html>").unwrap();
    let fallback = stage.join("index.html");
    fs::write(&fallback, "<html>fallback</html>").unwrap();
    let excluded = stage.join("Help/index.htm");
    fs::create_dir_all(excluded.parent().unwrap()).unwrap();
    fs::write(&excluded, "<html>help</html>").unwrap();
    let instance_path = stage.join("IM000001.dcm");
    dicom_image(&instance_path);
    let instance = document(
        instance_path,
        "dicom_file",
        "instance-4",
        Some("container-4"),
    );
    let mut person = person(temp.path());
    person.ingestion.exclude_patterns = vec!["**/Help/**".to_owned()];

    let studies = catalog_dicom_studies(&person, &[support, instance]);
    let support_study = studies
        .iter()
        .find(|study| study.support_kind == "dicom_iso")
        .unwrap();

    assert_eq!(
        support_study.html_viewer_path.as_deref(),
        Some(preferred.as_path())
    );
    assert!(support_study.viewer_paths.contains(&fallback));
    assert!(!support_study.viewer_paths.contains(&excluded));
}

#[test]
fn catalog_rejects_viewer_pages_with_external_runtime_dependencies() {
    let temp = TempDir::new();
    let support = document(
        temp.path().join("source/studio.iso"),
        "dicom_iso",
        "container-5",
        None,
    );
    let stage = temp.path().join("build/staging/containers/container-5");
    let page = stage.join("IHE_PDI/PAGES/STUDIES/STUDY1.HTM");
    fs::create_dir_all(page.parent().unwrap()).unwrap();
    fs::write(
        &page,
        "<script src='https://example.invalid/viewer.js'></script>",
    )
    .unwrap();
    let instance_path = stage.join("IM000001.dcm");
    dicom_image(&instance_path);
    let instance = document(
        instance_path,
        "dicom_file",
        "instance-5",
        Some("container-5"),
    );

    let studies = studies(temp.path(), &[support, instance]);

    let support_study = studies
        .iter()
        .find(|study| study.support_kind == "dicom_iso")
        .unwrap();
    assert_eq!(support_study.html_viewer_path, None);
}

#[test]
fn previews_decode_supported_pixels_and_preserve_the_original() {
    let temp = TempDir::new();
    let path = temp.path().join("image.dcm");
    dicom_image(&path);
    let original_bytes = fs::read(&path).unwrap();
    let person = person(temp.path());
    let document = document(path, "dicom_file", "image-id", None);
    let studies = catalog_dicom_studies(&person, &[document]);

    let previews = generate_dicom_previews(&person, &studies).unwrap();

    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].instance_count, 1, "{:?}", previews[0].warnings);
    let preview = person
        .local_build
        .join("dicom-previews")
        .join(&studies[0].study_id);
    let jpeg = fs::read(preview.join("00001.jpg")).unwrap();
    assert_eq!(&jpeg[..3], &[0xff, 0xd8, 0xff]);
    assert!(
        fs::read_to_string(preview.join("index.html"))
            .unwrap()
            .contains("non diagnostica")
    );
    assert_eq!(
        fs::read(&studies[0].support_paths[0]).unwrap(),
        original_bytes
    );
    assert!(
        person
            .local_build
            .join("manifests/dicom_previews.json")
            .is_file()
    );
}

#[test]
fn previews_report_unsupported_pixels_without_losing_the_instance() {
    let temp = TempDir::new();
    let path = temp.path().join("metadata-only.dcm");
    dicom_file(&path, Some("1.2.3"), None, None);
    let original_bytes = fs::read(&path).unwrap();
    let person = person(temp.path());
    let document = document(path, "dicom_file", "metadata-only", None);
    let studies = catalog_dicom_studies(&person, &[document]);

    let previews = generate_dicom_previews(&person, &studies).unwrap();

    assert_eq!(previews[0].instance_count, 0);
    assert!(!previews[0].warnings.is_empty());
    assert!(previews[0].warnings[0].contains("metadata-only.dcm"));
    assert_eq!(
        fs::read(&studies[0].support_paths[0]).unwrap(),
        original_bytes
    );
}
