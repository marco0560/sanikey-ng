//! Assemblaggio e verifica di immagini USB multi-persona.

#[cfg(test)]
mod tests;

use crate::config::{AccountsConfig, PersonConfig};
use crate::inventory::scan_document_inventory;
use crate::operation::{ErrorCode, OperationError};
use crate::{build::PatientBuildResult, version};
use chrono::Utc;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_STAGING: AtomicUsize = AtomicUsize::new(0);

/// Riepilogo di un'immagine USB completa per le persone abilitate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsbImageResult {
    /// Radice dell'immagine preparata.
    pub root: PathBuf,
    /// Manifest con gli checksum degli artefatti.
    pub manifest: PathBuf,
    /// Numero di persone incluse.
    pub patients: usize,
    /// Numero di file inclusi, manifest compreso.
    pub files: usize,
    /// Dimensione totale in byte.
    pub bytes: u64,
}

/// Assembla un'immagine multi-persona dai risultati build già completati.
///
/// Ogni persona abilitata deve avere un risultato build. I file vengono
/// assemblati in staging, verificati e poi rinominati alla destinazione; una
/// destinazione esistente o sovrapposta agli input viene rifiutata.
///
/// # Errors
///
/// Restituisce un errore se mancano build, gli input non sono leggibili, gli
/// alberi contengono link simbolici o l'immagine non supera la validazione.
pub fn assemble_usb_image(
    config: &AccountsConfig,
    builds: &[PatientBuildResult],
    destination: impl AsRef<Path>,
) -> Result<UsbImageResult, OperationError> {
    let destination = destination.as_ref();
    if !destination.is_absolute() {
        return Err(configuration_error(
            "la destinazione USB deve essere assoluta",
        ));
    }
    if destination.exists() || fs::symlink_metadata(destination).is_ok() {
        return Err(configuration_error(format!(
            "la destinazione dell'immagine esiste già: {}",
            destination.display()
        )));
    }
    let people = config.enabled_people().collect::<Vec<_>>();
    if people.is_empty() {
        return Err(configuration_error(
            "nessuna persona abilitata per l'export USB",
        ));
    }
    let mut build_by_person = BTreeMap::new();
    for build in builds {
        if build_by_person
            .insert(build.patient_id.as_str(), build)
            .is_some()
        {
            return Err(configuration_error(format!(
                "build duplicata per la persona {}",
                build.patient_id
            )));
        }
    }
    for person in &people {
        if !build_by_person.contains_key(person.id.as_str()) {
            return Err(configuration_error(format!(
                "build mancante per la persona {}",
                person.id
            )));
        }
        for input in [&person.source_documents, &person.local_build] {
            if overlaps(destination, input) {
                return Err(configuration_error(format!(
                    "destinazione USB sovrapposta agli input: {}",
                    destination.display()
                )));
            }
        }
    }

    let parent = destination.parent().ok_or_else(|| {
        configuration_error(format!(
            "destinazione senza directory padre: {}",
            destination.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        filesystem_error("impossibile creare la directory padre", parent, error)
    })?;
    let name = destination
        .file_name()
        .ok_or_else(|| configuration_error("nome destinazione USB mancante"))?
        .to_string_lossy();
    let staging = parent.join(format!(
        ".{name}.staging-{}-{}",
        std::process::id(),
        NEXT_STAGING.fetch_add(1, Ordering::Relaxed)
    ));
    if staging.exists() {
        return Err(configuration_error(format!(
            "directory di staging già esistente: {}",
            staging.display()
        )));
    }
    fs::create_dir(&staging)
        .map_err(|error| filesystem_error("impossibile creare lo staging USB", &staging, error))?;
    let assembled = assemble_staging(config, &people, &build_by_person, &staging);
    if assembled.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    let (manifest, files, bytes) = assembled?;
    if let Err(error) = validate_usb_image(&staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = fs::rename(&staging, destination) {
        let _ = fs::remove_dir_all(&staging);
        return Err(filesystem_error(
            "impossibile finalizzare l'immagine USB",
            destination,
            error,
        ));
    }
    Ok(UsbImageResult {
        root: destination.to_path_buf(),
        manifest: destination.join(manifest),
        patients: people.len(),
        files,
        bytes,
    })
}

/// Valida manifest, checksum, link, database e frontend di un'immagine USB.
///
/// # Errors
///
/// Restituisce un errore per un manifest invalido, file mancanti o alterati,
/// link assoluti/esterni o artefatti obbligatori assenti.
pub fn validate_usb_image(target: impl AsRef<Path>) -> Result<(), OperationError> {
    let target = target.as_ref();
    let metadata = fs::symlink_metadata(target)
        .map_err(|error| filesystem_error("directory immagine USB non leggibile", target, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(configuration_error(
            "la radice dell'immagine USB deve essere una directory reale",
        ));
    }
    let actual_files = collect_files(target)?;
    let manifest_path = target.join("SANIKEY-MANIFEST.json");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|error| filesystem_error("manifest USB non leggibile", &manifest_path, error))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| configuration_error(format!("manifest USB JSON invalido: {error}")))?;
    if manifest["schema_version"].as_i64() != Some(1) || !target.join("index.html").is_file() {
        return Err(configuration_error(
            "manifest USB o indice principale non valido",
        ));
    }
    let patients = manifest["patients"]
        .as_array()
        .filter(|patients| !patients.is_empty())
        .ok_or_else(|| configuration_error("elenco persone mancante nel manifest USB"))?;
    let mut ids = BTreeSet::new();
    for patient in patients {
        let id = patient["id"]
            .as_str()
            .ok_or_else(|| configuration_error("id persona mancante nel manifest USB"))?;
        let component = safe_relative_path(id)?;
        if component.components().count() != 1 || !ids.insert(id.to_owned()) {
            return Err(configuration_error(
                "id persona duplicato o non sicuro nel manifest USB",
            ));
        }
        validate_patient_links(target, id)?;
    }
    validate_root_links(target)?;
    let checksums = manifest["checksums"]
        .as_object()
        .ok_or_else(|| configuration_error("tabella checksum mancante nel manifest USB"))?;
    let listed = checksums.keys().cloned().collect::<BTreeSet<_>>();
    let actual = actual_files
        .into_iter()
        .filter(|path| path != &manifest_path)
        .map(|path| {
            path.strip_prefix(target)
                .map(normalize_relative)
                .map_err(|error| configuration_error(format!("file USB fuori radice: {error}")))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if listed != actual {
        return Err(configuration_error(
            "elenco file USB non corrispondente agli checksum del manifest",
        ));
    }
    for (relative, expected) in checksums {
        let relative = safe_relative_path(relative)?;
        let path = target.join(relative);
        if !path.is_file() || sha256_file(&path)? != expected.as_str().unwrap_or_default() {
            return Err(configuration_error(format!(
                "checksum USB mancante o diverso: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn assemble_staging(
    config: &AccountsConfig,
    people: &[&PersonConfig],
    builds: &BTreeMap<&str, &PatientBuildResult>,
    staging: &Path,
) -> Result<(PathBuf, usize, u64), OperationError> {
    let exported_at = Utc::now().to_rfc3339();
    for person in people {
        let patient_root = staging.join("patients").join(&person.id);
        fs::create_dir_all(&patient_root).map_err(|error| {
            filesystem_error(
                "impossibile creare l'archivio USB della persona",
                &patient_root,
                error,
            )
        })?;
        let build = builds[person.id.as_str()];
        copy_file(&build.database, &patient_root.join("medical_archive.db"))?;
        for directory in [
            "web",
            "medication-leaflets",
            "rendered-documents",
            "technical",
            "dicom-media",
            "dicom-previews",
        ] {
            copy_tree_if_present(
                &build.build_root.join(directory),
                &patient_root.join(directory),
            )?;
        }
        copy_source_documents(person, &patient_root.join("documents"))?;
        copy_dicom_viewers(&build.build_root, &patient_root.join("dicom-viewers"))?;
        write_usb_info(config, person, &patient_root, &exported_at)?;
    }
    write_root_index(people, staging)?;
    let mut checksums = BTreeMap::new();
    for path in collect_files(staging)? {
        let relative = path.strip_prefix(staging).map_err(|error| {
            configuration_error(format!("file USB fuori dalla radice: {error}"))
        })?;
        let relative = normalize_relative(relative);
        checksums.insert(relative, sha256_file(&path)?);
    }
    let manifest_path = staging.join("SANIKEY-MANIFEST.json");
    let manifest = json!({
        "schema_version": 1,
        "exported_at": exported_at,
        "sanikey_version": version(),
        "copy_strategy": config.usb.copy_strategy,
        "patients": people.iter().map(|person| json!({
            "id": person.id,
            "display_name": person.display_name,
            "usb_uuid": person.usb_uuid,
        })).collect::<Vec<_>>(),
        "checksums": checksums,
    });
    let mut encoded = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| configuration_error(format!("serializzazione manifest USB: {error}")))?;
    encoded.push(b'\n');
    fs::write(&manifest_path, encoded).map_err(|error| {
        filesystem_error(
            "impossibile scrivere il manifest USB",
            &manifest_path,
            error,
        )
    })?;
    let files = collect_files(staging)?;
    let mut bytes = 0_u64;
    for path in &files {
        bytes = bytes.saturating_add(
            fs::metadata(path)
                .map_err(|error| filesystem_error("metadati file USB non leggibili", path, error))?
                .len(),
        );
    }
    Ok((
        PathBuf::from("SANIKEY-MANIFEST.json"),
        files.len() + 1,
        bytes,
    ))
}

fn copy_source_documents(person: &PersonConfig, destination: &Path) -> Result<(), OperationError> {
    let inventory = scan_document_inventory(person)?;
    for document in inventory {
        let relative = document
            .path
            .strip_prefix(&person.source_documents)
            .map_err(|error| {
                configuration_error(format!(
                    "documento fuori dalla sorgente configurata: {error}"
                ))
            })?;
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(configuration_error(
                "percorso documento non sicuro nell'export USB",
            ));
        }
        copy_file(&document.path, &destination.join(relative))?;
    }
    Ok(())
}

fn copy_dicom_viewers(build_root: &Path, destination: &Path) -> Result<(), OperationError> {
    let manifest = build_root.join("manifests/dicom_html_viewers.json");
    if !manifest.is_file() {
        return Ok(());
    }
    let bytes = fs::read(&manifest).map_err(|error| {
        filesystem_error("manifest viewer DICOM non leggibile", &manifest, error)
    })?;
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|error| configuration_error(format!("manifest viewer DICOM invalido: {error}")))?;
    if let Some(viewers) = payload["viewers"].as_array() {
        for viewer in viewers {
            let (Some(study_id), Some(source_root), Some(relative_root)) = (
                viewer["study_id"].as_str(),
                viewer["source_root"].as_str(),
                viewer["relative_root"].as_str(),
            ) else {
                continue;
            };
            let study_id = safe_relative_path(study_id)?;
            let relative_root = safe_relative_path(relative_root)?;
            let source_root = Path::new(source_root);
            if source_root.is_dir() {
                copy_tree(source_root, &destination.join(study_id).join(relative_root))?;
            }
        }
    }
    Ok(())
}

fn write_usb_info(
    config: &AccountsConfig,
    person: &PersonConfig,
    patient_root: &Path,
    exported_at: &str,
) -> Result<(), OperationError> {
    let path = patient_root.join("web/usb-info.js");
    let payload = json!({
        "schema_version": 1,
        "usb_uuid": person.usb_uuid,
        "exported_at": exported_at,
        "sanikey_version": version(),
        "copy_strategy": config.usb.copy_strategy,
    });
    let serialized = serde_json::to_string(&payload)
        .map_err(|error| configuration_error(format!("serializzazione info USB: {error}")))?;
    let parent = path
        .parent()
        .ok_or_else(|| configuration_error("directory usb-info mancante"))?;
    fs::create_dir_all(parent)
        .map_err(|error| filesystem_error("directory frontend non creabile", parent, error))?;
    fs::write(&path, format!("window.SANIKEY_USB_INFO = {serialized};\n"))
        .map_err(|error| filesystem_error("impossibile scrivere usb-info.js", &path, error))
}

fn write_root_index(people: &[&PersonConfig], root: &Path) -> Result<(), OperationError> {
    let asset_dir = root.join("assets");
    fs::create_dir_all(&asset_dir)
        .map_err(|error| filesystem_error("directory asset USB non creabile", &asset_dir, error))?;
    fs::write(
        asset_dir.join("sanikey-logo.svg"),
        include_bytes!("../assets/frontend/assets/sanikey-logo-horizontal-transparent.svg"),
    )
    .map_err(|error| filesystem_error("impossibile scrivere il logo USB", &asset_dir, error))?;
    let links = people
        .iter()
        .map(|person| {
            format!(
                "<li><a href=\"patients/{}/web/index.html\">{}</a></li>",
                escape_html(&person.id),
                escape_html(&person.display_name)
            )
        })
        .collect::<String>();
    let index = root.join("index.html");
    let html = format!(
        "<!doctype html>\n<html lang=\"it\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>SaniKey</title></head><body><main><h1>SaniKey</h1><ul>{links}</ul><a href=\"https://github.com/marco0560/sanikey\"><img src=\"assets/sanikey-logo.svg\" alt=\"SaniKey\"></a></main></body></html>\n"
    );
    fs::write(&index, html)
        .map_err(|error| filesystem_error("impossibile scrivere l'indice USB", &index, error))
}

fn validate_patient_links(root: &Path, id: &str) -> Result<(), OperationError> {
    let patient = root.join("patients").join(id);
    for required in [
        patient.join("medical_archive.db"),
        patient.join("web/index.html"),
        patient.join("web/data.js"),
        patient.join("web/data/documents.json"),
    ] {
        if !required.is_file() {
            return Err(configuration_error(format!(
                "artefatto USB mancante: {}",
                required.display()
            )));
        }
    }
    let index = fs::read_to_string(patient.join("web/index.html"))
        .map_err(|error| filesystem_error("frontend USB non leggibile", &patient, error))?;
    if !index.contains("<html") {
        return Err(configuration_error(format!(
            "indice frontend invalido: {}",
            patient.display()
        )));
    }
    let data_path = patient.join("web/data.js");
    let payload = fs::read_to_string(&data_path)
        .map_err(|error| filesystem_error("dati frontend non leggibili", &data_path, error))?;
    let json_payload = payload
        .strip_prefix("window.SANIKEY_DATA = ")
        .and_then(|payload| payload.strip_suffix(";\n"))
        .ok_or_else(|| configuration_error("formato data.js frontend non valido"))?;
    let data: Value = serde_json::from_str(json_payload)
        .map_err(|error| configuration_error(format!("JSON frontend USB invalido: {error}")))?;
    let patient_root = patient.canonicalize().map_err(|error| {
        filesystem_error("directory persona USB non risolvibile", &patient, error)
    })?;
    for href in frontend_hrefs(&data) {
        validate_local_href(&patient.join("web"), &patient_root, href)?;
    }
    if let Some(therapies) = data
        .get("clinical")
        .and_then(|clinical| clinical.get("therapies"))
    {
        let therapies = therapies
            .as_array()
            .ok_or_else(|| configuration_error("elenco terapie frontend USB non valido"))?;
        for therapy in therapies {
            if therapy.get("non_aifa").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            for key in ["leaflet_href", "rcp_href"] {
                let href = therapy
                    .get(key)
                    .and_then(Value::as_str)
                    .ok_or_else(|| configuration_error(format!("link locale {key} mancante")))?;
                validate_local_href(&patient.join("web"), &patient_root, href)?;
            }
        }
    }
    Ok(())
}

fn validate_root_links(root: &Path) -> Result<(), OperationError> {
    let index = root.join("index.html");
    let content = fs::read_to_string(&index)
        .map_err(|error| filesystem_error("indice USB non leggibile", &index, error))?;
    for href in quoted_attribute_values(&content, "href=\"") {
        if href == "https://github.com/marco0560/sanikey" {
            continue;
        }
        validate_local_href(root, root, href)?;
    }
    for src in quoted_attribute_values(&content, "src=\"") {
        validate_local_href(root, root, src)?;
    }
    Ok(())
}

fn frontend_hrefs(value: &Value) -> Vec<&str> {
    match value {
        Value::Object(object) => object
            .iter()
            .flat_map(|(key, value)| {
                let mut hrefs = Vec::new();
                if key.ends_with("href")
                    && let Some(href) = value.as_str()
                {
                    hrefs.push(href);
                }
                hrefs.extend(frontend_hrefs(value));
                hrefs
            })
            .collect(),
        Value::Array(values) => values.iter().flat_map(frontend_hrefs).collect(),
        _ => Vec::new(),
    }
}

fn validate_local_href(base: &Path, allowed_root: &Path, href: &str) -> Result<(), OperationError> {
    if href.contains(':') || href.starts_with('/') || href.starts_with('\\') {
        return Err(configuration_error(format!(
            "link USB assoluto o esterno: {href}"
        )));
    }
    let path_part = href.split(['?', '#']).next().unwrap_or_default();
    let decoded = percent_decode(path_part)?;
    let mut target = base.to_path_buf();
    for component in Path::new(&decoded).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                target.pop();
            }
            Component::Normal(part) => target.push(part),
            _ => {
                return Err(configuration_error(format!(
                    "link USB non relativo: {href}"
                )));
            }
        }
    }
    if !target.starts_with(allowed_root) || !target.is_file() {
        return Err(configuration_error(format!(
            "link USB mancante o fuori radice: {href}"
        )));
    }
    Ok(())
}

fn quoted_attribute_values<'a>(html: &'a str, prefix: &str) -> Vec<&'a str> {
    html.match_indices(prefix)
        .filter_map(|(start, _)| {
            let value = &html[start + prefix.len()..];
            value.find('"').map(|end| &value[..end])
        })
        .collect()
}

fn percent_decode(value: &str) -> Result<String, OperationError> {
    let mut bytes = Vec::with_capacity(value.len());
    let input = value.as_bytes();
    let mut offset = 0;
    while offset < input.len() {
        if input[offset] == b'%' {
            let digits = input
                .get(offset + 1..offset + 3)
                .ok_or_else(|| configuration_error("encoding percent incompleto nel link USB"))?;
            let digits = std::str::from_utf8(digits)
                .map_err(|_| configuration_error("encoding percent invalido nel link USB"))?;
            bytes.push(
                u8::from_str_radix(digits, 16)
                    .map_err(|_| configuration_error("encoding percent invalido nel link USB"))?,
            );
            offset += 3;
        } else {
            bytes.push(input[offset]);
            offset += 1;
        }
    }
    String::from_utf8(bytes)
        .map_err(|_| configuration_error("percorso UTF-8 invalido nel link USB"))
}

fn copy_tree_if_present(source: &Path, target: &Path) -> Result<(), OperationError> {
    if source.is_dir() {
        copy_tree(source, target)?;
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), OperationError> {
    fs::create_dir_all(target)
        .map_err(|error| filesystem_error("directory export non creabile", target, error))?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| filesystem_error("directory build non leggibile", source, error))?
        .map(|entry| {
            entry.map_err(|error| filesystem_error("elemento build non leggibile", source, error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let kind = entry.file_type().map_err(|error| {
            filesystem_error("tipo elemento build non leggibile", &source_path, error)
        })?;
        if kind.is_symlink() {
            return Err(configuration_error(format!(
                "link simbolico non ammesso nell'export USB: {}",
                source_path.display()
            )));
        } else if kind.is_dir() {
            copy_tree(&source_path, &target_path)?;
        } else if kind.is_file() {
            copy_file(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn copy_file(source: &Path, target: &Path) -> Result<(), OperationError> {
    let parent = target
        .parent()
        .ok_or_else(|| configuration_error("file export senza directory padre"))?;
    fs::create_dir_all(parent)
        .map_err(|error| filesystem_error("directory export non creabile", parent, error))?;
    fs::copy(source, target)
        .map_err(|error| filesystem_error("copia artefatto USB fallita", target, error))?;
    Ok(())
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>, OperationError> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| filesystem_error("directory export non leggibile", &directory, error))?
            .map(|entry| {
                entry.map_err(|error| {
                    filesystem_error("elemento export non leggibile", &directory, error)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let kind = entry.file_type().map_err(|error| {
                filesystem_error("tipo file export non leggibile", &path, error)
            })?;
            if kind.is_symlink() {
                return Err(configuration_error(format!(
                    "link simbolico non ammesso nell'immagine USB: {}",
                    path.display()
                )));
            } else if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn sha256_file(path: &Path) -> Result<String, OperationError> {
    let mut file = File::open(path)
        .map_err(|error| filesystem_error("file USB non leggibile", path, error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let length = file
            .read(&mut buffer)
            .map_err(|error| filesystem_error("lettura file USB fallita", path, error))?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn safe_relative_path(value: &str) -> Result<PathBuf, OperationError> {
    if value.is_empty() || value.contains('\\') || value.starts_with('/') {
        return Err(configuration_error(format!(
            "percorso USB non relativo: {value}"
        )));
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(configuration_error(format!(
            "percorso USB non sicuro: {value}"
        )));
    }
    Ok(path.to_path_buf())
}

fn normalize_relative(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn filesystem_error(context: &str, path: &Path, error: std::io::Error) -> OperationError {
    OperationError::new(
        ErrorCode::Configuration,
        format!("{context} {}: {error}", path.display()),
    )
}

fn configuration_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}
