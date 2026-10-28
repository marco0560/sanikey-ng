//! Esportazioni JSON e JavaScript della consultazione statica.

use crate::config::PersonConfig;
use crate::dicom::DicomStudy;
use crate::metadata::{CuratedMetadata, DocumentRecord, TimelineEvent};
use crate::operation::{ErrorCode, OperationError};
use crate::pdf::ExtractedText;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[cfg(test)]
mod tests;

/// Percorsi degli export generati per una singola persona.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportResult {
    /// Directory con i dati statici della consultazione.
    pub data_dir: PathBuf,
    /// Catalogo documenti JSON.
    pub documents: PathBuf,
    /// Indice di ricerca JSON.
    pub search: PathBuf,
    /// Cronologia JSON.
    pub timeline: PathBuf,
    /// Sommario JSON.
    pub summary: PathBuf,
    /// Dati completi caricabili da `file://`.
    pub data_script: PathBuf,
    /// Indice testuale avanzato caricabile da `file://`.
    pub content_search_script: PathBuf,
    /// Avvisi non bloccanti sulla dimensione dell'indice avanzato.
    pub warning_messages: Vec<String>,
}

/// Genera gli export di consultazione per una persona senza includere percorsi assoluti.
///
/// I record e gli studi con un identificatore di persona diverso da `person.id`
/// vengono esclusi, così gli input condivisi fra build non mescolano i dati.
/// I documenti non apribili vengono elencati nei report tecnici.
///
/// # Errors
///
/// Restituisce un errore se un documento Markdown, l'indice avanzato o uno degli
/// artefatti JSON, JavaScript, CSV o HTML non può essere letto o scritto.
pub fn generate_exports(
    person: &PersonConfig,
    documents: &[DocumentRecord],
    metadata: &CuratedMetadata,
    dicom_studies: &[DicomStudy],
    extracted_text: &[ExtractedText],
) -> Result<ExportResult, OperationError> {
    let documents: Vec<_> = documents
        .iter()
        .filter(|document| document.patient_id == person.id)
        .cloned()
        .collect();
    let dicom_studies: Vec<_> = dicom_studies
        .iter()
        .filter(|study| study.patient_id == person.id)
        .cloned()
        .collect();
    let data_dir = person.local_build.join("web/data");
    let technical_container_ids: Vec<_> = dicom_studies
        .iter()
        .filter(|study| study.support_path.starts_with(&person.source_documents))
        .filter_map(|study| {
            documents
                .iter()
                .find(|document| document.path == study.support_path)
        })
        .map(|document| document.document_id.as_str())
        .collect();
    let consultation: Vec<_> = documents
        .iter()
        .filter(|document| {
            !document
                .container_id
                .as_deref()
                .is_some_and(|id| technical_container_ids.contains(&id))
        })
        .filter(|document| consultation_href(person, document).is_some())
        .filter(|document| !document.kind.starts_with("dicom_"))
        .collect();
    let mut ordered = consultation.clone();
    ordered.sort_by(|left, right| {
        right
            .date
            .cmp(&left.date)
            .then_with(|| right.title.cmp(&left.title))
            .then_with(|| left.path.cmp(&right.path))
    });

    let document_payloads: Vec<_> = ordered
        .iter()
        .map(|document| document_payload(person, document, metadata, &dicom_studies))
        .collect::<Result<_, _>>()?;
    let clinical = clinical_payload(person, metadata, &dicom_studies);
    let mut search: Vec<_> = consultation
        .iter()
        .map(|document| document_search_payload(person, document, metadata, &dicom_studies))
        .collect();
    search.extend(metadata_search_payloads(&clinical));
    let timeline = timeline_payload(&consultation, metadata, &person.ui.timeline_order);
    let background_image = person.ui.background_image.as_ref().map(|path| {
        let extension = path
            .extension()
            .and_then(|part| part.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        format!(
            "assets/background{}",
            if extension.is_empty() {
                String::new()
            } else {
                format!(".{extension}")
            }
        )
    });
    let summary = json!({
        "patient_id": person.id,
        "display_name": person.display_name,
        "ui": {
            "accent_color": person.ui.accent_color,
            "density": person.ui.density,
            "default_tab": person.ui.default_tab,
            "timeline_order": person.ui.timeline_order,
            "document_link_mode": person.ui.document_link_mode,
            "subtitle": person.ui.subtitle,
            "background_image": background_image,
            "background_opacity": person.ui.background_opacity,
        },
        "document_count": consultation.len(),
        "problem_count": metadata.problems.len(),
        "therapy_count": metadata.therapies.len(),
        "procedure_count": metadata.procedures.len(),
        "observation_count": metadata.observations.len(),
        "clinical_summary": metadata.clinical_summary,
        "clinical_summary_html": metadata.clinical_summary.as_deref().map(markdown_summary),
    });

    let documents_path = write_json(
        &data_dir.join("documents.json"),
        &Value::Array(document_payloads.clone()),
    )?;
    let search_path = write_json(&data_dir.join("search.json"), &Value::Array(search.clone()))?;
    let timeline_path = write_json(
        &data_dir.join("timeline.json"),
        &Value::Array(timeline.clone()),
    )?;
    let summary_path = write_json(&data_dir.join("summary.json"), &summary)?;
    let data_script = write_script(
        &person.local_build.join("web/data.js"),
        "SANIKEY_DATA",
        &json!({
            "documents": document_payloads,
            "clinical": clinical,
            "search": search,
            "timeline": timeline,
            "summary": summary,
        }),
    )?;
    let text_by_id: HashMap<_, _> = extracted_text
        .iter()
        .filter(|record| !record.text.is_empty())
        .map(|record| (record.document_id.as_str(), record.text.as_str()))
        .collect();
    let content_search = json!({
        "schema_version": 1,
        "dictionary": {
            "terms": person.search.dictionary_data.terms.iter().collect::<std::collections::BTreeMap<_, _>>(),
            "months": person.search.dictionary_data.months.iter().collect::<std::collections::BTreeMap<_, _>>(),
        },
        "documents": ordered.iter().filter_map(|document| {
            text_by_id.get(document.document_id.as_str()).map(|text| {
                let mut payload = document_payload(person, document, metadata, &dicom_studies)?;
                payload["text"] = json!(text);
                Ok(payload)
            })
        }).collect::<Result<Vec<_>, OperationError>>()?,
    });
    let content_search_script = write_script(
        &person.local_build.join("web/content-search.js"),
        "SANIKEY_CONTENT_SEARCH",
        &content_search,
    )?;
    let size_mb = fs::metadata(&content_search_script)
        .map_err(|error| {
            export_error(format!(
                "lettura dimensione indice ricerca {}: {error}",
                content_search_script.display()
            ))
        })?
        .len() as f64
        / (1024.0 * 1024.0);
    let warning_messages = if size_mb > person.search.advanced_index_warning_mb as f64 {
        vec![format!(
            "indice ricerca avanzata grande: {size_mb:.1} MiB supera la soglia avviso configurata {} MiB",
            person.search.advanced_index_warning_mb
        )]
    } else {
        Vec::new()
    };
    let technical_documents: Vec<_> = documents
        .iter()
        .filter(|document| {
            !document.kind.starts_with("dicom_")
                && (document
                    .container_id
                    .as_deref()
                    .is_some_and(|id| technical_container_ids.contains(&id))
                    || consultation_href(person, document).is_none())
        })
        .collect();
    write_technical_documents(person, &technical_documents)?;
    write_dicom_viewer_manifest(person, &dicom_studies)?;
    write_json(
        &person.local_build.join("search/search.json"),
        &Value::Array(search),
    )?;
    write_json(
        &person.local_build.join("timeline/timeline.json"),
        &Value::Array(timeline),
    )?;
    Ok(ExportResult {
        data_dir,
        documents: documents_path,
        search: search_path,
        timeline: timeline_path,
        summary: summary_path,
        data_script,
        content_search_script,
        warning_messages,
    })
}

fn document_payload(
    person: &PersonConfig,
    document: &DocumentRecord,
    metadata: &CuratedMetadata,
    dicom_studies: &[DicomStudy],
) -> Result<Value, OperationError> {
    let href = consultation_href(person, document);
    let viewer = dicom_studies_for_document(document, dicom_studies);
    let mut payload = json!({
        "id": document.document_id,
        "title": document.title,
        "date": document.date,
        "category": document.category,
        "kind": document.kind,
        "path": display_path(person, document),
        "href": href,
        "source_href": source_href(person, document),
        "origin": document.origin,
        "container_id": document.container_id,
        "internal_path": document.internal_path,
        "tags": resolved_tags(document, metadata),
        "markdown_html": if document.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("md")) { Some(format!("<pre>{}</pre>", escape_html(&fs::read_to_string(&document.path).map_err(|error| export_error(format!("lettura Markdown {}: {error}", document.path.display())))?))) } else { None },
    });
    if let Some((study_id, viewer_href)) = viewer {
        payload["viewer_href"] = json!(viewer_href);
        payload["dicom_study_id"] = json!(study_id);
        payload["support_href"] = json!(href);
        payload["primary_href"] = json!(viewer_href);
        payload["primary_action"] = json!("Apri studio DICOM");
    }
    Ok(payload)
}

fn document_search_payload(
    person: &PersonConfig,
    document: &DocumentRecord,
    metadata: &CuratedMetadata,
    dicom_studies: &[DicomStudy],
) -> Value {
    let tags = resolved_tags(document, metadata);
    let viewer = dicom_studies_for_document(document, dicom_studies);
    let tag_text = tags.join(" ");
    let text = join_present([
        document.title.as_str(),
        document.category.as_str(),
        tag_text.as_str(),
    ]);
    let mut payload = json!({
        "id": document.document_id,
        "type": "document",
        "section": "documents",
        "title": document.title,
        "subtitle": join_present([document.date.as_deref().unwrap_or(""), document.category.as_str(), document.kind.as_str()]),
        "date": document.date,
        "text": text,
        "tags": tags,
        "fields": visible_fields(vec![("Categoria", json!(document.category)), ("Tipo", json!(document.kind)), ("Percorso", json!(display_path(person, document)))]),
        "href": source_href(person, document),
        "origin": document.origin,
        "container_id": document.container_id,
    });
    if let Some((study_id, viewer_href)) = viewer {
        payload["viewer_href"] = json!(viewer_href);
        payload["dicom_study_id"] = json!(study_id);
        payload["support_href"] = payload["href"].clone();
        payload["primary_href"] = json!(viewer_href);
        payload["primary_action"] = json!("Apri studio DICOM");
    }
    payload
}

fn clinical_payload(
    person: &PersonConfig,
    metadata: &CuratedMetadata,
    studies: &[DicomStudy],
) -> Value {
    let leaflet_by_id: HashMap<_, _> = metadata
        .medication_leaflets
        .iter()
        .map(|item| (item.medication_id.as_str(), item))
        .collect();
    let excluded: Vec<_> = metadata
        .medication_leaflet_exclusions
        .iter()
        .map(|item| item.medication_id.as_str())
        .collect();
    let medications: Vec<_> = metadata.medications.iter().map(|medication| {
        let mut payload = json!({
            "id": medication.id, "type": "medication", "title": medication.name,
            "active_ingredient": medication.active_ingredient, "form": medication.form,
            "strength_per_unit": medication.strength_per_unit,
            "text": join_present([medication.name.as_str(), medication.active_ingredient.as_deref().unwrap_or(""), medication.form.as_deref().unwrap_or(""), medication.strength_per_unit.as_deref().unwrap_or("")]),
            "fields": visible_fields(vec![("Principio attivo", json!(medication.active_ingredient)), ("Forma", json!(medication.form)), ("Dosaggio per unita'", json!(medication.strength_per_unit))]),
            "non_aifa": excluded.contains(&medication.id.as_str()),
        });
        if let Some(reference) = leaflet_by_id.get(medication.id.as_str()) {
            let directory = person.local_build.join("medication-leaflets").join(&medication.id);
            let fi = directory.join("foglio-illustrativo.pdf");
            let rcp = directory.join("rcp.pdf");
            payload["leaflet_href"] = json!(fi.is_file().then(|| format!("../medication-leaflets/{}/foglio-illustrativo.pdf", medication.id)));
            payload["rcp_href"] = json!(rcp.is_file().then(|| format!("../medication-leaflets/{}/rcp.pdf", medication.id)));
            payload["leaflet_downloaded_at"] = json!(download_date(person, &medication.id).or_else(|| reference.downloaded_at.clone()));
            payload["aifa_fi_url"] = json!(aifa_url(reference, "FI"));
            payload["aifa_rcp_url"] = json!(aifa_url(reference, "RCP"));
        }
        payload
    }).collect();
    let medication_payloads: HashMap<_, _> = medications
        .iter()
        .filter_map(|item| item["id"].as_str().map(|id| (id, item)))
        .collect();
    json!({
        "problems": metadata.problems.iter().map(|item| json!({"id":item.id,"type":"problem","title":item.title,"status":item.status,"text":join_present([item.title.as_str(),item.status.as_str()]),"fields":[{"label":"Stato","value":item.status}]})).collect::<Vec<_>>(),
        "medications": medications,
        "therapies": metadata.therapies.iter().map(|item| {
            let medication = medication_payloads.get(item.medication_id.as_str());
            let name = medication.and_then(|m| m["title"].as_str()).unwrap_or(&item.medication_id);
            let active = medication.and_then(|m| m["active_ingredient"].as_str());
            let schedule = item.schedule.join(", ");
            json!({"id":item.id,"type":"therapy","title":format!("Terapia: {name}"),"medication_id":item.medication_id,"medication_name":name,"active_ingredient":active,"start_date":item.start_date,"end_date":item.end_date,"date":item.start_date,"dosage":item.dosage,"role":item.role,"schedule":item.schedule,"instructions":item.instructions,"text":join_present([item.id.as_str(),name,active.unwrap_or(""),item.medication_id.as_str(),item.start_date.as_deref().unwrap_or(""),item.end_date.as_deref().unwrap_or(""),item.dosage.as_deref().unwrap_or(""),item.role.as_deref().unwrap_or(""),schedule.as_str(),item.instructions.as_deref().unwrap_or("")]),"fields":visible_fields(vec![("Farmaco",json!(name)),("Principio attivo",json!(active)),("Dosaggio",json!(item.dosage)),("Schedula",json!(schedule)),("Istruzioni",json!(item.instructions)),("Ruolo",json!(item.role)),("Inizio",json!(item.start_date)),("Fine",json!(item.end_date))]),"leaflet_href":medication.map(|m|m["leaflet_href"].clone()).unwrap_or(Value::Null),"rcp_href":medication.map(|m|m["rcp_href"].clone()).unwrap_or(Value::Null),"leaflet_downloaded_at":medication.map(|m|m["leaflet_downloaded_at"].clone()).unwrap_or(Value::Null),"aifa_fi_url":medication.map(|m|m["aifa_fi_url"].clone()).unwrap_or(Value::Null),"aifa_rcp_url":medication.map(|m|m["aifa_rcp_url"].clone()).unwrap_or(Value::Null),"non_aifa":medication.is_some_and(|m|m["non_aifa"]==true)})
        }).collect::<Vec<_>>(),
        "procedures": metadata.procedures.iter().map(|item| json!({"id":item.id,"type":"procedure","title":item.title,"date":item.date,"status":item.status,"text":join_present([item.title.as_str(),item.date.as_deref().unwrap_or(""),item.status.as_str()]),"fields":visible_fields(vec![("Data",json!(item.date)),("Stato",json!(item.status))])})).collect::<Vec<_>>(),
        "observations": metadata.observations.iter().map(|item| json!({"id":item.id,"type":"observation","title":item.kind,"date":item.date,"value":item.value,"text":join_present([item.kind.as_str(),item.value.as_str(),item.date.as_deref().unwrap_or("")]),"fields":visible_fields(vec![("Valore",json!(item.value)),("Data",json!(item.date))])})).collect::<Vec<_>>(),
        "observation_series": metadata.observation_series.iter().map(|item| json!({"id":item.id,"type":"observation_series","title":item.name,"name":item.name,"value_type":item.value_type,"unit":item.unit,"description":item.description,"synonyms":item.synonyms,"text":join_present([item.name.as_str(),item.synonyms.join(" ").as_str(),item.value_type.as_str(),item.unit.as_deref().unwrap_or("")])})).collect::<Vec<_>>(),
        "observation_points": metadata.observation_points.iter().map(|item| {
            let value = point_value(item);
            let title = metadata.observation_series.iter().find(|series| series.id == item.series_id).map_or(item.series_id.as_str(), |series| series.name.as_str());
            json!({"id":item.id,"type":"observation_point","title":title,"series_id":item.series_id,"date":item.observation_date,"value":value,"numeric_value":item.numeric_value,"systolic":item.systolic,"diastolic":item.diastolic,"pulse":item.pulse,"source_reference":item.source_reference,"note":item.note,"source_kind":item.source_kind,"document_id":item.document_id,"document_href":item.document_href,"document_title":item.document_title,"document_category":item.document_category,"matched_label":item.matched_label,"raw_value":item.raw_value,"parsed_value":item.parsed_value,"raw_unit":item.raw_unit,"normalized_unit":item.normalized_unit,"qualifier":item.qualifier,"rule_id":item.rule_id,"rule_version":item.rule_version,"reason_code":item.reason_code,"text":join_present([item.series_id.as_str(),item.observation_date.as_str(),value.as_str(),item.source_reference.as_str(),item.note.as_deref().unwrap_or("")])})
        }).collect::<Vec<_>>(),
        "dicom_studies": studies.iter().filter(|study| study.study_instance_uid.is_some() || ["dicom_study","dicomdir_study"].contains(&study.support_kind.as_str()) || study.html_viewer_path.is_some()).map(|study| {
            let title = study.study_description.as_deref().unwrap_or_else(|| study.support_path.file_name().and_then(|s|s.to_str()).unwrap_or(""));
            let href = source_href_from_path(person, &study.support_path);
            let native_viewer = dicom_viewer_href(study);
            let preview = dicom_preview_href(person, study);
            let viewer = native_viewer.clone().or_else(|| preview.clone());
            let media = dicom_media_href(person, study);
            let support_name=study.support_path.file_name().and_then(|s|s.to_str()).unwrap_or("");
            let count=study.instance_count.to_string();
            json!({"id":study.study_id,"type":"dicom_study","title":title,"date":study.study_date,"support_kind":study.support_kind,"support_path":support_name,"study_instance_uid":study.study_instance_uid,"study_description":study.study_description,"instance_count":study.instance_count,"href":href,"viewer_href":viewer,"native_viewer_href":native_viewer,"preview_href":preview,"dicomdir_href":media,"text":join_present([title,support_name,study.support_kind.as_str(),study.study_date.as_deref().unwrap_or(""),study.study_instance_uid.as_deref().unwrap_or(""),if viewer.is_some(){"viewer html"}else{"viewer html non rilevato"},count.as_str()]),"fields":visible_fields(vec![("Supporto",json!(support_name)),("Tipo",json!(study.support_kind)),("Data",json!(study.study_date)),("UID",json!(study.study_instance_uid)),("Istanze",json!(count)),("Viewer HTML",json!(if native_viewer.is_some(){"nativo"}else if preview.is_some(){"anteprima"}else{"non rilevato"})),("Anomalia",json!(if viewer.is_none(){Some("anteprima non disponibile")}else{None}))])})
        }).collect::<Vec<_>>(),
    })
}

fn metadata_search_payloads(clinical: &Value) -> Vec<Value> {
    let mapping = [
        ("problems", "problems"),
        ("medications", "medications"),
        ("therapies", "therapies"),
        ("procedures", "procedures"),
        ("observations", "observations"),
        ("observation_series", "observations"),
        ("observation_points", "observations"),
        ("dicom_studies", "dicom"),
    ];
    mapping.into_iter().flat_map(|(key, section)| clinical[key].as_array().into_iter().flatten().map(move |item| {
        let mut result = json!({"id":item["id"],"type":item["type"],"section":section,"title":item["title"],"subtitle":entity_subtitle(item),"date":item.get("date").filter(|v|!v.is_null()).or_else(||item.get("start_date")).cloned().unwrap_or(Value::Null),"text":item["text"],"tags":[],"fields":item.get("fields").cloned().unwrap_or(json!([]))});
        for key in ["href","viewer_href","dicomdir_href","primary_href","primary_action"] { if !item[key].is_null() { result[key]=item[key].clone(); } }
        result
    })).collect()
}

fn timeline_payload(
    documents: &[&DocumentRecord],
    metadata: &CuratedMetadata,
    order: &str,
) -> Vec<Value> {
    let mut events = metadata.timeline_events.clone();
    events.extend(documents.iter().filter_map(|document| {
        document.date.as_ref().map(|date| TimelineEvent {
            id: format!("document-{}", document.document_id),
            title: document.title.clone(),
            start_date: Some(date.clone()),
            end_date: None,
            source: "document".to_owned(),
            links: vec![document.document_id.clone()],
        })
    }));
    events.extend(metadata.therapies.iter().filter_map(|therapy| {
        therapy.start_date.as_ref().map(|date| {
            let name = metadata
                .medications
                .iter()
                .find(|med| med.id == therapy.medication_id)
                .map_or(therapy.medication_id.as_str(), |med| med.name.as_str());
            TimelineEvent {
                id: format!("therapy-{}", therapy.id),
                title: format!("Terapia: {name}"),
                start_date: Some(date.clone()),
                end_date: therapy.end_date.clone(),
                source: "therapy".to_owned(),
                links: vec![therapy.id.clone(), therapy.medication_id.clone()],
            }
        })
    }));
    events.sort_by(|left, right| {
        left.start_date
            .cmp(&right.start_date)
            .then_with(|| left.title.cmp(&right.title))
    });
    if order != "asc" {
        events.reverse();
    }
    events.into_iter().map(|event|json!({"id":event.id,"title":event.title,"start_date":event.start_date,"end_date":event.end_date,"source":event.source,"links":event.links})).collect()
}

fn resolved_tags(document: &DocumentRecord, metadata: &CuratedMetadata) -> Vec<String> {
    let path = document.path.to_string_lossy().replace('\\', "/");
    metadata
        .document_tags
        .iter()
        .find(|(key, _)| {
            path.ends_with(&format!("/{}", key.replace('\\', "/")))
                || document.path.file_name().and_then(|part| part.to_str()) == Some(key.as_str())
        })
        .map_or_else(|| document.tags.clone(), |(_, tags)| tags.clone())
}

fn consultation_href(person: &PersonConfig, document: &DocumentRecord) -> Option<String> {
    let rendered = person
        .local_build
        .join("rendered-documents")
        .join(&document.document_id);
    if let Ok(entries) = fs::read_dir(rendered) {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        paths.sort();
        if let Some(path) = paths.first() {
            return Some(format!(
                "../rendered-documents/{}/{}",
                document.document_id,
                path.file_name()?.to_string_lossy()
            ));
        }
    }
    let extension = document
        .path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if document.origin != "source"
        || !["pdf", "txt", "md", "htm", "html", "jpg", "jpeg", "png"].contains(&extension.as_str())
    {
        return None;
    }
    source_href(person, document)
}

fn dicom_studies_for_document(
    document: &DocumentRecord,
    studies: &[DicomStudy],
) -> Option<(String, String)> {
    studies.iter().find_map(|study| {
        if study.support_path != document.path {
            return None;
        }
        Some((study.study_id.clone(), dicom_viewer_href(study)?))
    })
}

fn dicom_viewer_href(study: &DicomStudy) -> Option<String> {
    let viewer = study.html_viewer_path.as_ref()?;
    let extracted = study.extracted_path.as_ref()?;
    let relative_parts: Vec<_> = viewer.strip_prefix(extracted).ok()?.components().collect();
    let ihe_index = relative_parts.iter().position(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("ihe_pdi")
    });
    let relative = if let Some(index) = ihe_index {
        relative_parts[index..].iter().collect::<PathBuf>()
    } else {
        PathBuf::from(viewer.parent()?.file_name()?).join(viewer.file_name()?)
    };
    Some(format!(
        "../dicom-viewers/{}/{}",
        study.study_id,
        relative.to_string_lossy().replace('\\', "/")
    ))
}

fn dicom_preview_href(person: &PersonConfig, study: &DicomStudy) -> Option<String> {
    person
        .local_build
        .join("dicom-previews")
        .join(&study.study_id)
        .join("index.html")
        .is_file()
        .then(|| format!("../dicom-previews/{}/index.html", study.study_id))
}

fn dicom_media_href(person: &PersonConfig, study: &DicomStudy) -> Option<String> {
    let manifest = person.local_build.join("manifests/dicom_media.json");
    let payload: Value = serde_json::from_slice(&fs::read(manifest).ok()?).ok()?;
    payload["media"].as_array()?.iter().find_map(|entry| {
        let includes_study = entry["study_ids"]
            .as_array()?
            .iter()
            .any(|id| id.as_str() == Some(study.study_id.as_str()));
        if !includes_study {
            return None;
        }
        let media_id = entry["media_id"].as_str()?;
        person
            .local_build
            .join("dicom-media")
            .join(media_id)
            .join("DICOMDIR")
            .is_file()
            .then(|| format!("../dicom-media/{media_id}/DICOMDIR"))
    })
}

fn write_technical_documents(
    person: &PersonConfig,
    documents: &[&DocumentRecord],
) -> Result<(), OperationError> {
    let technical = person.local_build.join("technical");
    let headers = [
        "estensione",
        "nome",
        "origine",
        "contenitore",
        "percorso",
        "motivo",
    ];
    let mut ordered = documents.to_vec();
    ordered.sort_by_key(|document| {
        (
            document
                .path
                .extension()
                .map_or_else(String::new, |part| part.to_string_lossy().to_lowercase()),
            display_path(person, document).to_lowercase(),
        )
    });
    let rows: Vec<[String; 6]> = ordered
        .iter()
        .map(|document| {
            [
                document.path.extension().map_or_else(
                    || "(nessuna)".to_owned(),
                    |part| format!(".{}", part.to_string_lossy().to_lowercase()),
                ),
                document
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                document.origin.clone(),
                document.internal_path.clone().unwrap_or_default(),
                display_path(person, document),
                "nessuna rappresentazione apribile disponibile".to_owned(),
            ]
        })
        .collect();
    let mut csv = String::new();
    for row in std::iter::once(headers.map(str::to_owned)).chain(rows.iter().cloned()) {
        if !csv.is_empty() {
            csv.push('\n');
        }
        csv.push_str(
            &row.iter()
                .map(|cell| format!("\"{}\"", cell.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(";"),
        );
    }
    csv.push('\n');
    write_bytes(
        &technical.join("documenti-non-apribili.csv"),
        csv.as_bytes(),
    )?;
    let mut html = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>Documenti non apribili</title><h1>Documenti non apribili</h1><table><thead><tr>",
    );
    for header in headers {
        html.push_str(&format!("<th>{}</th>", escape_html(header)));
    }
    html.push_str("</tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr>");
        for cell in row {
            html.push_str(&format!("<td>{}</td>", escape_html(&cell)));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    write_bytes(
        &technical.join("documenti-non-apribili.html"),
        html.as_bytes(),
    )
}

fn write_dicom_viewer_manifest(
    person: &PersonConfig,
    studies: &[DicomStudy],
) -> Result<(), OperationError> {
    let viewers: Vec<_> = studies.iter().filter_map(|study| {
        let viewer = study.html_viewer_path.as_ref()?;
        let extracted = study.extracted_path.as_ref()?;
        let relative = viewer.strip_prefix(extracted).ok()?;
        let components: Vec<_> = relative.components().collect();
        let ihe_index = components.iter().position(|part| part.as_os_str().to_string_lossy().eq_ignore_ascii_case("ihe_pdi"));
        let root = ihe_index.map_or_else(|| viewer.parent().unwrap_or(extracted).to_path_buf(), |index| extracted.join(components[..=index].iter().collect::<PathBuf>()));
        let root_parent = root.parent()?;
        let entrypoint = viewer.strip_prefix(root_parent).ok()?;
        Some(json!({
            "study_id": study.study_id,
            "source_root": root,
            "relative_root": root.strip_prefix(root_parent).ok()?.to_string_lossy().replace('\\', "/"),
            "entrypoint": entrypoint.to_string_lossy().replace('\\', "/"),
        }))
    }).collect();
    write_json(
        &person.local_build.join("manifests/dicom_html_viewers.json"),
        &json!({"schema_version":1,"viewers":viewers}),
    )?;
    Ok(())
}

fn source_href(person: &PersonConfig, document: &DocumentRecord) -> Option<String> {
    source_href_from_path(person, &document.path)
}
fn source_href_from_path(person: &PersonConfig, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(&person.source_documents).ok()?;
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    Some(format!(
        "../documents/{}",
        relative.to_string_lossy().replace('\\', "/")
    ))
}
fn display_path(person: &PersonConfig, document: &DocumentRecord) -> String {
    if document.origin == "source" {
        return source_href(person, document).map_or_else(
            || {
                document
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            },
            |href| href.trim_start_matches("../documents/").to_owned(),
        );
    }
    document
        .internal_path
        .as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            document
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
}
fn join_present<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
fn visible_fields(fields: Vec<(&str, Value)>) -> Vec<Value> {
    fields
        .into_iter()
        .filter(|(_, value)| !value.is_null() && value.as_str().is_none_or(|text| !text.is_empty()))
        .map(|(label, value)| json!({"label":label,"value":value}))
        .collect()
}
fn entity_subtitle(item: &Value) -> String {
    item["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .take(3)
        .filter_map(|field| field["value"].as_str())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}
fn point_value(point: &crate::metadata::ObservationPoint) -> String {
    if let (Some(systolic), Some(diastolic)) = (point.systolic, point.diastolic) {
        let base = format!("{}/{}", format_number(systolic), format_number(diastolic));
        return point.pulse.map_or(base.clone(), |pulse| {
            format!("{base} FC {}", format_number(pulse))
        });
    }
    point.numeric_value.map_or_else(
        || point.text_value.clone().unwrap_or_default(),
        format_number,
    )
}
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}
fn aifa_url(reference: &crate::metadata::MedicationLeaflet, kind: &str) -> String {
    format!(
        "https://api.aifa.gov.it/aifa-bdf-eif-be/1.0.0/organizzazione/{}/farmaci/{}/stampati?ts={kind}",
        reference.codice_sis, reference.aic6
    )
}
fn download_date(person: &PersonConfig, medication_id: &str) -> Option<String> {
    let path = person
        .local_build
        .join("manifests/medication-leaflets.json");
    let bytes = fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("downloaded_at")?
        .get(medication_id)?
        .as_str()
        .map(str::to_owned)
}
fn markdown_summary(value: &str) -> String {
    format!("<p>{}</p>", escape_html(value).replace('\n', "<br>"))
}
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn write_json(path: &Path, value: &Value) -> Result<PathBuf, OperationError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| export_error(format!("serializzazione JSON: {error}")))?;
    let mut output = bytes;
    output.push(b'\n');
    write_bytes(path, &output)?;
    Ok(path.to_owned())
}
fn write_script(path: &Path, name: &str, value: &Value) -> Result<PathBuf, OperationError> {
    let mut bytes = format!("window.{name} = ").into_bytes();
    bytes.extend(serialize_python_json(value)?);
    bytes.extend_from_slice(b";\n");
    write_bytes(path, &bytes)?;
    Ok(path.to_owned())
}

fn serialize_python_json(value: &Value) -> Result<Vec<u8>, OperationError> {
    let serialized = match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            serde_json::to_string(value)
                .map_err(|error| export_error(format!("serializzazione JavaScript: {error}")))?
        }
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(serialize_python_json)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(key, value)| {
                    let key = serde_json::to_string(key).map_err(|error| {
                        export_error(format!("serializzazione chiave JavaScript: {error}"))
                    })?;
                    let value = serialize_python_json(value)?;
                    Ok(format!("{key}: {}", String::from_utf8_lossy(&value)))
                })
                .collect::<Result<Vec<_>, OperationError>>()?
                .join(", ")
        ),
    };
    Ok(serialized.into_bytes())
}
fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), OperationError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            export_error(format!(
                "creazione directory export {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::write(path, bytes)
        .map_err(|error| export_error(format!("scrittura export {}: {error}", path.display())))
}
fn export_error(message: String) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}
