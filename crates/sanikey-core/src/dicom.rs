//! Catalogazione degli studi DICOM senza decodifica delle immagini.

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::metadata::DocumentRecord;
use crate::operation::{ErrorCode, OperationError};
use dicom_core::Tag;
use dicom_object::{DicomAttribute, DicomObject, file::OpenFileOptions};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const STUDY_INSTANCE_UID: Tag = Tag(0x0020, 0x000d);
const STUDY_DATE: Tag = Tag(0x0008, 0x0020);
const STUDY_DESCRIPTION: Tag = Tag(0x0008, 0x1030);
const DIRECTORY_RECORD_SEQUENCE: Tag = Tag(0x0004, 0x1220);
const DIRECTORY_RECORD_TYPE: Tag = Tag(0x0004, 0x1430);
const REFERENCED_FILE_ID: Tag = Tag(0x0004, 0x1500);
const PIXEL_DATA: Tag = Tag(0x7fe0, 0x0010);

/// Metadati DICOM usati per identificare e descrivere uno studio.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DicomMetadata {
    /// Study Instance UID, quando presente e leggibile.
    pub study_instance_uid: Option<String>,
    /// Data normalizzata a `YYYY-MM-DD` quando la forma DICOM è valida.
    pub study_date: Option<String>,
    /// Descrizione leggibile dello studio.
    pub study_description: Option<String>,
}

/// Uno studio DICOM catalogato con i percorsi che ne documentano l'origine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DicomStudy {
    /// Identificatore SHA-256 stabile, derivato da paziente e UID o documento.
    pub study_id: String,
    /// Persona proprietaria dello studio.
    pub patient_id: String,
    /// Percorso del supporto originale preferito.
    pub support_path: PathBuf,
    /// Tipo del supporto o record che ha originato lo studio.
    pub support_kind: String,
    /// Directory di staging, se le istanze derivano da un contenitore.
    pub extracted_path: Option<PathBuf>,
    /// File simili a viewer individuati sotto il supporto estratto.
    pub viewer_paths: Vec<PathBuf>,
    /// Pagina HTML compatibile scelta come viewer preferito.
    pub html_viewer_path: Option<PathBuf>,
    /// Avvisi non bloccanti prodotti dalla lettura dei media.
    pub warnings: Vec<String>,
    /// Study Instance UID DICOM.
    pub study_instance_uid: Option<String>,
    /// Data DICOM dello studio, quando disponibile.
    pub study_date: Option<String>,
    /// Descrizione DICOM dello studio, quando disponibile.
    pub study_description: Option<String>,
    /// Numero di istanze leggibili raggruppate nello studio.
    pub instance_count: usize,
    /// Percorsi delle istanze DICOM che contribuiscono allo studio.
    pub support_paths: Vec<PathBuf>,
}

/// Media DICOM standardizzato preparato da un supporto espanso.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DicomMedia {
    /// Identificatore stabile del supporto derivato.
    pub media_id: String,
    /// Studi presenti nel supporto.
    pub study_ids: Vec<String>,
    /// Directory di output del media.
    pub directory: PathBuf,
    /// Istanze DICOM copiate byte per byte.
    pub instance_count: usize,
    /// Anomalie non bloccanti del media.
    pub warnings: Vec<String>,
}

/// Esito delle anteprime statiche per uno studio.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DicomPreview {
    /// Identificatore dello studio.
    pub study_id: String,
    /// Numero di JPEG generati.
    pub instance_count: usize,
    /// Anomalie di decodifica o scrittura.
    pub warnings: Vec<String>,
}

/// Cataloga i supporti DICOM, raggruppa le istanze e conserva la provenienza.
///
/// `documents` può contenere sia record originali sia record dei membri
/// prodotti da [`crate::containers::stage_container_documents`]. Il catalogo
/// legge solo i metadati necessari; non decodifica Pixel Data né certifica la
/// leggibilità delle immagini.
///
/// # Examples
///
/// ```
/// use sanikey_core::dicom::catalog_dicom_studies;
/// # use sanikey_core::config::{PersonConfig, IngestionConfig, SearchConfig, UiConfig};
/// # use sanikey_core::metadata::DocumentRecord;
/// # use std::path::PathBuf;
/// # let person = PersonConfig { id: "persona".into(), display_name: "Persona".into(), source_documents: PathBuf::new(), metadata_directory: PathBuf::new(), local_build: PathBuf::new(), usb_uuid: String::new(), enabled: true, ui: UiConfig::default(), search: SearchConfig::default(), ingestion: IngestionConfig::default() };
/// let studies = catalog_dicom_studies(&person, &[]);
/// assert!(studies.is_empty());
/// ```
#[must_use]
pub fn catalog_dicom_studies(
    person: &PersonConfig,
    documents: &[DocumentRecord],
) -> Vec<DicomStudy> {
    let support_documents: HashMap<_, _> = documents
        .iter()
        .map(|document| (document.document_id.as_str(), document))
        .collect();
    let mut studies = Vec::new();
    let mut dicom_files = Vec::new();

    for document in documents {
        if document
            .path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("DICOMDIR"))
        {
            let directory_studies = studies_from_dicomdir(person, document);
            if directory_studies.is_empty() {
                dicom_files.push(document);
            } else {
                studies.extend(directory_studies);
            }
        } else if document.kind == "dicom_file" {
            dicom_files.push(document);
        } else if is_dicom_support(document) {
            let extracted_path = documents
                .iter()
                .filter(|child| child.container_id.as_deref() == Some(&document.document_id))
                .min_by(|left, right| left.path.cmp(&right.path))
                .and_then(|child| staging_root(&child.path, &document.document_id));
            let (viewer_paths, html_viewer_path) = extracted_path
                .as_deref()
                .map(|root| discover_viewers(person, root))
                .unwrap_or_default();
            studies.push(DicomStudy {
                study_id: document.document_id.clone(),
                patient_id: document.patient_id.clone(),
                support_path: document.path.clone(),
                support_kind: document.kind.clone(),
                extracted_path: extracted_path.clone(),
                viewer_paths,
                html_viewer_path,
                warnings: if extracted_path.is_some() {
                    Vec::new()
                } else {
                    vec!["contenitore DICOM non espanso".to_owned()]
                },
                study_instance_uid: None,
                study_date: None,
                study_description: None,
                instance_count: 1,
                support_paths: Vec::new(),
            });
        }
    }

    let mut grouped: BTreeMap<String, Vec<(&DocumentRecord, DicomMetadata)>> = BTreeMap::new();
    let mut fallback = Vec::new();
    for document in dicom_files {
        match read_dicom_metadata(&document.path) {
            Some(metadata) if metadata.study_instance_uid.is_some() => {
                if let Some(uid) = metadata.study_instance_uid.clone() {
                    grouped.entry(uid).or_default().push((document, metadata));
                }
            }
            Some(_) => fallback.push(fallback_study(
                document,
                "istanza DICOM senza Study Instance UID",
            )),
            None => fallback.push(fallback_study(document, "metadati DICOM non leggibili")),
        }
    }

    for (uid, mut instances) in grouped {
        instances.sort_by(|left, right| left.0.path.cmp(&right.0.path));
        let first = &instances[0];
        let instance_paths: Vec<_> = instances
            .iter()
            .map(|(document, _)| document.path.clone())
            .collect();
        let support = instances
            .iter()
            .filter_map(|(document, _)| document.container_id.as_deref())
            .filter_map(|id| support_documents.get(id).copied())
            .min_by(|left, right| left.path.cmp(&right.path));
        let extracted_path = support.and_then(|container| {
            instances
                .iter()
                .find(|(document, _)| {
                    document.container_id.as_deref() == Some(&container.document_id)
                })
                .and_then(|(document, _)| staging_root(&document.path, &container.document_id))
        });
        studies.push(DicomStudy {
            study_id: study_id(&person.id, &uid),
            patient_id: person.id.clone(),
            support_path: support
                .map_or_else(|| first.0.path.clone(), |record| record.path.clone()),
            support_kind: "dicom_study".to_owned(),
            extracted_path,
            viewer_paths: Vec::new(),
            html_viewer_path: None,
            warnings: Vec::new(),
            study_instance_uid: Some(uid),
            study_date: first.1.study_date.clone(),
            study_description: first.1.study_description.clone(),
            instance_count: instances.len(),
            support_paths: instance_paths,
        });
    }
    studies.extend(fallback);
    coalesce_and_link(studies)
}

/// Copia istanze e DICOMDIR validabili e conserva i viewer HTML relativi.
///
/// Gli originali non vengono spostati o modificati. Un DICOMDIR assente o non
/// valido produce un avviso e non interrompe la copia delle istanze.
///
/// # Errors
///
/// Restituisce un errore se non è possibile preparare la directory di output
/// o scrivere il manifest.
pub fn prepare_dicom_media(
    person: &PersonConfig,
    studies: &[DicomStudy],
) -> Result<Vec<DicomMedia>, OperationError> {
    let target_root = person.local_build.join("dicom-media");
    reset_directory(&target_root)?;
    let mut grouped: BTreeMap<PathBuf, Vec<&DicomStudy>> = BTreeMap::new();
    for study in studies {
        if let Some(source) = study.extracted_path.as_ref().filter(|path| path.is_dir()) {
            grouped.entry(source.clone()).or_default().push(study);
        }
    }

    let mut entries = Vec::new();
    for (source, source_studies) in grouped {
        let relative = source.strip_prefix(&person.local_build).unwrap_or(&source);
        let media_id = short_digest(relative.to_string_lossy().as_bytes());
        let destination = target_root.join(&media_id);
        fs::create_dir_all(&destination).map_err(|error| {
            filesystem_error("impossibile creare il media DICOM", &destination, error)
        })?;
        let mut files = Vec::new();
        collect_files(&source, &mut files);
        files.sort();
        let viewer_roots = source_studies
            .iter()
            .filter_map(|study| study.html_viewer_path.as_deref())
            .map(|path| viewer_root(&source, path))
            .collect::<Vec<_>>();
        let mut copied = 0;
        let mut dicomdir_copied = false;
        let mut warnings = Vec::new();

        for path in files {
            let relative = match path.strip_prefix(&source) {
                Ok(relative) => relative,
                Err(_) => continue,
            };
            let is_dicomdir = path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("DICOMDIR"));
            let is_instance = is_dicom_instance_path(&path);
            let is_viewer_asset = viewer_roots.iter().any(|root| path.starts_with(root))
                && !is_executable_or_installer(&path);
            if is_dicomdir {
                if validate_dicomdir(&path, &source) {
                    let target = destination.join(relative);
                    dicomdir_copied = copy_file(&path, &target, &mut warnings);
                } else {
                    warnings.push(format!(
                        "{}: DICOMDIR illeggibile o con riferimenti immagine non risolti",
                        path.display()
                    ));
                }
            } else if is_instance || is_viewer_asset {
                let target = destination.join(relative);
                let copied_ok = copy_file(&path, &target, &mut warnings);
                if is_instance && copied_ok {
                    copied += 1;
                }
            }
        }
        if copied > 0 && !dicomdir_copied {
            warnings.push(format!(
                "{}: DICOMDIR assente; istanze originali conservate senza indice standard",
                source.display()
            ));
        } else if copied == 0 {
            warnings.push(format!(
                "{}: supporto senza istanze DICOM leggibili",
                source.display()
            ));
        }
        let mut study_ids = source_studies
            .iter()
            .map(|study| study.study_id.clone())
            .collect::<Vec<_>>();
        study_ids.sort();
        study_ids.dedup();
        entries.push(DicomMedia {
            media_id,
            study_ids,
            directory: destination,
            instance_count: copied,
            warnings,
        });
    }

    let manifest = person
        .local_build
        .join("manifests")
        .join("dicom_media.json");
    write_json_manifest(
        &manifest,
        serde_json::json!({
            "schema_version": 1,
            "media": entries.iter().map(|entry| serde_json::json!({
                "media_id": entry.media_id,
                "study_ids": entry.study_ids,
                "instance_count": entry.instance_count,
                "warnings": entry.warnings,
            })).collect::<Vec<_>>(),
        }),
    )?;
    Ok(entries)
}

/// Genera anteprime JPEG statiche e non diagnostiche per gli studi leggibili.
///
/// Si converte il primo frame in scala di grigi. Sintassi di trasferimento o
/// istanze non decodificabili restano nel media standard e producono avvisi.
///
/// # Errors
///
/// Restituisce un errore se non è possibile preparare il percorso di output o
/// il manifest delle anteprime.
pub fn generate_dicom_previews(
    person: &PersonConfig,
    studies: &[DicomStudy],
) -> Result<Vec<DicomPreview>, OperationError> {
    use dicom_pixeldata::PixelDecoder;
    use image::ImageEncoder;
    use image::codecs::jpeg::JpegEncoder;

    let root = person.local_build.join("dicom-previews");
    reset_directory(&root)?;
    let mut previews = Vec::new();
    for study in studies {
        let mut paths = study.support_paths.clone();
        if paths.is_empty()
            && let Some(extracted) = study.extracted_path.as_ref()
        {
            collect_files(extracted, &mut paths);
        }
        paths.sort();
        paths.dedup();
        let target = root.join(&study.study_id);
        fs::create_dir_all(&target).map_err(|error| {
            filesystem_error("impossibile creare le anteprime DICOM", &target, error)
        })?;
        let mut images = Vec::new();
        let mut warnings = Vec::new();
        for (index, path) in paths
            .iter()
            .filter(|path| {
                !path
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("DICOMDIR"))
                    && is_dicom_instance_path(path)
            })
            .enumerate()
        {
            if study.study_instance_uid.as_deref().is_some_and(|uid| {
                read_dicom_metadata(path).and_then(|metadata| metadata.study_instance_uid)
                    != Some(uid.to_owned())
            }) {
                continue;
            }
            let result = (|| -> Result<(), String> {
                let object = OpenFileOptions::new()
                    .open_file(path)
                    .map_err(|error| error.to_string())?;
                let decoded = object
                    .decode_pixel_data()
                    .map_err(|error| error.to_string())?;
                let image = decoded
                    .to_dynamic_image(0)
                    .map_err(|error| error.to_string())?
                    .to_luma8();
                let name = format!("{:05}.jpg", index + 1);
                let target_path = target.join(&name);
                let file = fs::File::create(&target_path).map_err(|error| error.to_string())?;
                JpegEncoder::new_with_quality(file, 95)
                    .write_image(
                        &image,
                        image.width(),
                        image.height(),
                        image::ExtendedColorType::L8,
                    )
                    .map_err(|error| error.to_string())?;
                images.push(name);
                Ok(())
            })();
            if let Err(error) = result {
                warnings.push(format!(
                    "{}: anteprima DICOM non generata: {error}",
                    path.display()
                ));
            }
        }
        warnings.sort();
        warnings.dedup();
        if !images.is_empty() {
            let html = preview_html(&images);
            fs::write(target.join("index.html"), html).map_err(|error| {
                filesystem_error("impossibile scrivere la pagina anteprime", &target, error)
            })?;
        }
        previews.push(DicomPreview {
            study_id: study.study_id.clone(),
            instance_count: images.len(),
            warnings,
        });
    }
    previews.sort_by(|left, right| left.study_id.cmp(&right.study_id));
    let manifest = person
        .local_build
        .join("manifests")
        .join("dicom_previews.json");
    write_json_manifest(
        &manifest,
        serde_json::json!({
            "schema_version": 1,
            "previews": previews.iter().map(|preview| serde_json::json!({
                "study_id": preview.study_id,
                "instances": preview.instance_count,
                "warnings": preview.warnings,
            })).collect::<Vec<_>>(),
        }),
    )?;
    Ok(previews)
}

fn is_dicom_support(document: &DocumentRecord) -> bool {
    matches!(
        document.kind.as_str(),
        "dicom_img" | "dicom_iso" | "dicom_7z" | "dicom_rar" | "dicom_tar_xz" | "dicom_zip"
    )
}

fn fallback_study(document: &DocumentRecord, warning: &str) -> DicomStudy {
    DicomStudy {
        study_id: document.document_id.clone(),
        patient_id: document.patient_id.clone(),
        support_path: document.path.clone(),
        support_kind: document.kind.clone(),
        extracted_path: None,
        viewer_paths: Vec::new(),
        html_viewer_path: None,
        warnings: vec![warning.to_owned()],
        study_instance_uid: None,
        study_date: None,
        study_description: None,
        instance_count: 1,
        support_paths: vec![document.path.clone()],
    }
}

fn read_dicom_metadata(path: &Path) -> Option<DicomMetadata> {
    let object = OpenFileOptions::new()
        .read_until(PIXEL_DATA)
        .open_file(path)
        .ok()?;
    Some(DicomMetadata {
        study_instance_uid: element_text(&object, STUDY_INSTANCE_UID),
        study_date: element_text(&object, STUDY_DATE).map(normalize_date),
        study_description: element_text(&object, STUDY_DESCRIPTION),
    })
}

fn studies_from_dicomdir(person: &PersonConfig, document: &DocumentRecord) -> Vec<DicomStudy> {
    let Ok(object) = OpenFileOptions::new().open_file(&document.path) else {
        return Vec::new();
    };
    let Some(items) = object
        .element(DIRECTORY_RECORD_SEQUENCE)
        .ok()
        .and_then(|element| element.value().items())
    else {
        return Vec::new();
    };
    let mut by_uid = BTreeMap::new();
    for item in items {
        if element_text(&item, DIRECTORY_RECORD_TYPE).as_deref() != Some("STUDY") {
            continue;
        }
        let metadata = DicomMetadata {
            study_instance_uid: element_text(&item, STUDY_INSTANCE_UID),
            study_date: element_text(&item, STUDY_DATE).map(normalize_date),
            study_description: element_text(&item, STUDY_DESCRIPTION),
        };
        if let Some(uid) = metadata.study_instance_uid.clone() {
            by_uid.entry(uid).or_insert(metadata);
        }
    }
    by_uid
        .into_iter()
        .map(|(uid, metadata)| DicomStudy {
            study_id: study_id(&person.id, &uid),
            patient_id: person.id.clone(),
            support_path: document.path.clone(),
            support_kind: "dicomdir_study".to_owned(),
            extracted_path: document.path.parent().map(Path::to_path_buf),
            viewer_paths: Vec::new(),
            html_viewer_path: None,
            warnings: Vec::new(),
            study_instance_uid: Some(uid),
            study_date: metadata.study_date,
            study_description: metadata.study_description,
            instance_count: 0,
            support_paths: Vec::new(),
        })
        .collect()
}

fn element_text(object: &impl DicomObject, tag: Tag) -> Option<String> {
    object
        .attr_opt(tag)
        .ok()??
        .to_str()
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_date(value: String) -> String {
    if value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        format!("{}-{}-{}", &value[..4], &value[4..6], &value[6..])
    } else {
        value
    }
}

fn study_id(patient_id: &str, uid: &str) -> String {
    let identity = format!("dicom-study\0{patient_id}\0{uid}");
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn staging_root(path: &Path, container_id: &str) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| {
            ancestor
                .file_name()
                .is_some_and(|name| name == container_id)
                && ancestor
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "containers")
        })
        .map(Path::to_path_buf)
}

fn discover_viewers(person: &PersonConfig, root: &Path) -> (Vec<PathBuf>, Option<PathBuf>) {
    let mut paths = Vec::new();
    collect_files(root, &mut paths);
    paths.retain(|path| !is_excluded_path(person, root, path));
    paths.sort();
    let viewer_paths = paths
        .iter()
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                matches!(
                    name.to_string_lossy().to_ascii_lowercase().as_str(),
                    "viewer.exe" | "start.exe" | "autorun.inf" | "index.html" | "index.htm"
                )
            })
        })
        .cloned()
        .collect();
    let html = paths
        .iter()
        .filter(|path| {
            path.extension().is_some_and(|extension| {
                matches!(
                    extension.to_string_lossy().to_ascii_lowercase().as_str(),
                    "htm" | "html"
                )
            })
        })
        .filter(|path| viewer_is_offline_compatible(path))
        .min_by_key(|path| (viewer_priority(root, path), (*path).clone()))
        .filter(|path| viewer_priority(root, path) < 3)
        .cloned();
    (viewer_paths, html)
}

fn viewer_is_offline_compatible(path: &Path) -> bool {
    let Ok(source) = fs::read_to_string(path) else {
        return false;
    };
    let source = source.to_ascii_lowercase();
    !["http://", "https://", "fetch(", "xmlhttprequest"]
        .iter()
        .any(|forbidden| source.contains(forbidden))
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_files(&path, files);
        } else if file_type.is_file() {
            files.push(path);
        }
    }
}

fn is_dicom_instance_path(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    if file.seek(SeekFrom::Start(128)).is_err() {
        return false;
    }
    let mut marker = [0; 4];
    file.read_exact(&mut marker).is_ok() && marker == *b"DICM"
}

fn is_excluded_path(person: &PersonConfig, root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    crate::inventory::is_ingestion_excluded(
        &person.ingestion,
        &relative.to_string_lossy().replace('\\', "/"),
    )
}

fn viewer_priority(root: &Path, path: &Path) -> u8 {
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    if relative.contains("ihe_pdi/pages/studies/") {
        0
    } else if relative.contains("ihe_pdi/pages/") {
        1
    } else if path.file_name().is_some_and(|name| {
        matches!(
            name.to_string_lossy().to_ascii_lowercase().as_str(),
            "index.html" | "index.htm" | "default.htm" | "start.htm"
        )
    }) {
        2
    } else {
        3
    }
}

fn viewer_root(extracted: &Path, html: &Path) -> PathBuf {
    if let Some(root) = html.ancestors().find(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("IHE_PDI"))
    }) {
        return root.to_path_buf();
    }
    if html.file_name().is_some_and(|name| {
        matches!(
            name.to_string_lossy().to_ascii_lowercase().as_str(),
            "index.html" | "index.htm"
        )
    }) && html.parent() == Some(extracted)
    {
        return extracted.to_path_buf();
    }
    html.parent().unwrap_or(extracted).to_path_buf()
}

fn is_executable_or_installer(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        matches!(
            extension.to_string_lossy().to_ascii_lowercase().as_str(),
            "app" | "bat" | "cmd" | "com" | "dll" | "dmg" | "exe" | "jar" | "msi" | "sh"
        )
    }) || path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("autorun.inf"))
}

fn validate_dicomdir(path: &Path, source_root: &Path) -> bool {
    let Ok(object) = OpenFileOptions::new().open_file(path) else {
        return false;
    };
    let Some(items) = object
        .element(DIRECTORY_RECORD_SEQUENCE)
        .ok()
        .and_then(|element| element.value().items())
    else {
        return false;
    };
    let mut image_records = 0;
    for item in items {
        if element_text(item, DIRECTORY_RECORD_TYPE).as_deref() != Some("IMAGE") {
            continue;
        }
        image_records += 1;
        let Some(referenced) = element_text(item, REFERENCED_FILE_ID) else {
            return false;
        };
        let mut target = source_root.to_path_buf();
        for component in referenced.split('\\') {
            if component.is_empty() || matches!(component, "." | "..") || component.contains('/') {
                return false;
            }
            target.push(component);
        }
        if !target.is_file() || !is_dicom_instance_path(&target) {
            return false;
        }
    }
    image_records > 0
}

fn copy_file(source: &Path, target: &Path, warnings: &mut Vec<String>) -> bool {
    if let Some(parent) = target.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        warnings.push(format!(
            "{}: directory media non creata: {error}",
            parent.display()
        ));
        return false;
    }
    match fs::copy(source, target) {
        Ok(_) => true,
        Err(error) => {
            warnings.push(format!(
                "{}: copia nel media non riuscita: {error}",
                source.display()
            ));
            false
        }
    }
}

fn reset_directory(path: &Path) -> Result<(), OperationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => {
            fs::remove_file(path).map_err(|error| {
                filesystem_error("impossibile rimuovere l'output precedente", path, error)
            })?;
        }
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(path).map_err(|error| {
                filesystem_error("impossibile sostituire l'output precedente", path, error)
            })?;
        }
        Ok(_) => {
            return Err(OperationError::new(
                ErrorCode::Configuration,
                format!("tipo di output non supportato: {}", path.display()),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(filesystem_error(
                "impossibile esaminare il percorso di output",
                path,
                error,
            ));
        }
    }
    fs::create_dir_all(path)
        .map_err(|error| filesystem_error("impossibile creare la directory di output", path, error))
}

fn filesystem_error(message: &str, path: &Path, cause: std::io::Error) -> OperationError {
    OperationError::new(
        ErrorCode::Configuration,
        format!("{message}: {}: {cause}", path.display()),
    )
}

fn write_json_manifest(path: &Path, value: serde_json::Value) -> Result<(), OperationError> {
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|error| {
        OperationError::new(
            ErrorCode::Configuration,
            format!("manifest DICOM non serializzabile: {error}"),
        )
    })?;
    bytes.push(b'\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            filesystem_error(
                "impossibile creare la directory dei manifest",
                parent,
                error,
            )
        })?;
    }
    fs::write(path, bytes)
        .map_err(|error| filesystem_error("impossibile scrivere il manifest", path, error))
}

fn short_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))[..16].to_owned()
}

fn preview_html(images: &[String]) -> String {
    let image_list = serde_json::to_string(images).unwrap_or_else(|_| "[]".to_owned());
    format!(
        "<!doctype html><html lang=\"it\"><meta charset=\"utf-8\"><title>Anteprima DICOM</title><style>body{{font-family:sans-serif;margin:1rem;max-width:1100px}}img{{max-width:100%;max-height:78vh;display:block;margin:auto}}button{{margin:.5rem}}</style><h1>Anteprima DICOM non diagnostica</h1><p>Usare il DICOMDIR con un lettore professionale per valutazioni cliniche.</p><button id=\"previous\">Precedente</button><button id=\"next\">Successiva</button><span id=\"count\"></span><img id=\"image\" alt=\"Istanza DICOM\"><script>const images={image_list};let index=0;const image=document.querySelector('#image');const count=document.querySelector('#count');function show(){{image.src=images[index];count.textContent=`${{index+1}} / ${{images.length}}`;}}document.querySelector('#previous').onclick=()=>{{index=(index+images.length-1)%images.length;show();}};document.querySelector('#next').onclick=()=>{{index=(index+1)%images.length;show();}};show();</script></html>"
    )
}

fn coalesce_and_link(mut studies: Vec<DicomStudy>) -> Vec<DicomStudy> {
    studies.sort_by(|left, right| {
        (left.instance_count == 0, &left.support_path)
            .cmp(&(right.instance_count == 0, &right.support_path))
    });
    let mut positions = HashMap::new();
    let mut merged: Vec<DicomStudy> = Vec::new();
    for study in studies {
        if let Some(index) = positions.get(&study.study_id).copied() {
            let existing: &mut DicomStudy = &mut merged[index];
            if existing.extracted_path.is_none() {
                existing.extracted_path = study.extracted_path;
            }
            if existing.study_date.is_none() {
                existing.study_date = study.study_date;
            }
            if existing.study_description.is_none() {
                existing.study_description = study.study_description;
            }
            existing.instance_count = existing.instance_count.max(study.instance_count);
            append_unique(&mut existing.viewer_paths, study.viewer_paths);
            if existing.html_viewer_path.is_none() {
                existing.html_viewer_path = study.html_viewer_path;
            }
            append_unique(&mut existing.support_paths, study.support_paths);
            append_unique(&mut existing.warnings, study.warnings);
        } else {
            positions.insert(study.study_id.clone(), merged.len());
            merged.push(study);
        }
    }
    let mut result = merged;
    let supports: Vec<_> = result
        .iter()
        .filter(|study| study.extracted_path.is_some())
        .cloned()
        .collect();
    for study in &mut result {
        if study.extracted_path.is_some() || study.support_paths.is_empty() {
            continue;
        }
        if let Some(support) = supports
            .iter()
            .filter(|support| {
                study.support_paths.iter().any(|path| {
                    support
                        .extracted_path
                        .as_ref()
                        .is_some_and(|root| path.starts_with(root))
                })
            })
            .min_by(|left, right| left.support_path.cmp(&right.support_path))
        {
            study.extracted_path = support.extracted_path.clone();
            append_unique(&mut study.viewer_paths, support.viewer_paths.clone());
            if study.html_viewer_path.is_none() {
                study.html_viewer_path = support.html_viewer_path.clone();
            }
            if study.support_kind == "dicomdir_study" {
                study.support_path = support.support_path.clone();
            }
        }
    }
    result.sort_by(|left, right| left.support_path.cmp(&right.support_path));
    result
}

fn append_unique<T: Eq>(target: &mut Vec<T>, source: Vec<T>) {
    for item in source {
        if !target.contains(&item) {
            target.push(item);
        }
    }
}
