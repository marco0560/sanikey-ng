//! Catalogazione degli studi DICOM senza decodifica delle immagini.

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::metadata::DocumentRecord;
use dicom_core::Tag;
use dicom_object::{DicomAttribute, DicomObject, file::OpenFileOptions};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const STUDY_INSTANCE_UID: Tag = Tag(0x0020, 0x000d);
const STUDY_DATE: Tag = Tag(0x0008, 0x0020);
const STUDY_DESCRIPTION: Tag = Tag(0x0008, 0x1030);
const DIRECTORY_RECORD_SEQUENCE: Tag = Tag(0x0004, 0x1220);
const DIRECTORY_RECORD_TYPE: Tag = Tag(0x0004, 0x1430);
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
            studies.push(DicomStudy {
                study_id: document.document_id.clone(),
                patient_id: document.patient_id.clone(),
                support_path: document.path.clone(),
                support_kind: document.kind.clone(),
                extracted_path: extracted_path.clone(),
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
