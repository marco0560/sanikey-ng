//! Esportazione e validazione del campione TXT su directory simulata.

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::operation::{ErrorCode, OperationError};
use crate::txt_pipeline::build_txt_pipeline;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_STAGE: AtomicUsize = AtomicUsize::new(0);

/// Risultato dell'esportazione del sottoinsieme TXT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulatedExportResult {
    /// Radice della directory esportata.
    pub root: PathBuf,
    /// Manifest con gli SHA-256 degli artefatti.
    pub manifest: PathBuf,
    /// Numero di persone esportate.
    pub patients: usize,
    /// Numero totale di file, incluso il manifest.
    pub files: usize,
}

struct StageDirectory {
    path: PathBuf,
    committed: bool,
}

impl Drop for StageDirectory {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Crea una directory autonoma con database, frontend, documenti TXT e manifest.
///
/// La destinazione deve essere nuova e simulata. Il generatore non riconosce né
/// scrive su volumi fisici. L'esportazione è costruita in una directory di
/// staging e rinominata alla destinazione solo dopo validazione completa.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` se la destinazione esiste, si
/// sovrappone agli input o se copia, checksum o validazione falliscono.
pub fn export_txt_simulation(
    person: &PersonConfig,
    target: impl AsRef<Path>,
) -> Result<SimulatedExportResult, OperationError> {
    let target = absolute_path(target.as_ref())?;
    reject_input_overlap(person, &target)?;
    if target.exists() || fs::symlink_metadata(&target).is_ok() {
        return Err(configuration_error(format!(
            "la destinazione simulata esiste già: {}",
            target.display()
        )));
    }
    let pipeline = build_txt_pipeline(person)?;
    let parent = target.parent().ok_or_else(|| {
        configuration_error(format!(
            "la destinazione non ha una directory padre: {}",
            target.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|cause| {
        filesystem_error("impossibile creare la directory padre", parent, cause)
    })?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            configuration_error(format!(
                "nome destinazione non valido: {}",
                target.display()
            ))
        })?;
    let stage_path = parent.join(format!(
        ".{file_name}.staging-{}-{}",
        std::process::id(),
        NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&stage_path)
        .map_err(|cause| filesystem_error("impossibile creare lo staging", &stage_path, cause))?;
    let mut stage = StageDirectory {
        path: stage_path,
        committed: false,
    };

    let patient_root = stage.path.join("patients").join(&person.id);
    fs::create_dir_all(&patient_root).map_err(|cause| {
        filesystem_error(
            "impossibile creare l'archivio persona",
            &patient_root,
            cause,
        )
    })?;
    copy_file(&pipeline.database, &patient_root.join("medical_archive.db"))?;
    copy_tree(&person.local_build.join("web"), &patient_root.join("web"))?;
    copy_tree(
        &person.local_build.join("documents"),
        &patient_root.join("documents"),
    )?;
    write_root_index(&stage.path, person)?;
    let checksums = checksum_tree(&stage.path)?;
    write_manifest(&stage.path, person, checksums)?;
    validate_txt_simulation(&stage.path)?;
    let files = count_files(&stage.path)?;

    fs::rename(&stage.path, &target).map_err(|cause| {
        filesystem_error("impossibile finalizzare l'esportazione", &target, cause)
    })?;
    stage.committed = true;
    let manifest = target.join("SANIKEY-MANIFEST.json");
    Ok(SimulatedExportResult {
        root: target,
        manifest,
        patients: 1,
        files,
    })
}

/// Convalida manifest, checksum e link locali del sottoinsieme TXT.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` se manca un artefatto richiesto,
/// il manifest non è valido, un checksum non coincide o un link esce
/// dall'archivio della persona.
pub fn validate_txt_simulation(target: impl AsRef<Path>) -> Result<(), OperationError> {
    let target = target.as_ref();
    let manifest_path = target.join("SANIKEY-MANIFEST.json");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|cause| filesystem_error("manifest non leggibile", &manifest_path, cause))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).map_err(|cause| {
        configuration_error(format!(
            "manifest JSON non valido {}: {cause}",
            manifest_path.display()
        ))
    })?;
    if manifest["schema_version"].as_i64() != Some(1) {
        return Err(configuration_error("versione manifest non supportata"));
    }
    if !target.join("index.html").is_file() {
        return Err(configuration_error("indice principale mancante"));
    }
    let patients = manifest["patients"]
        .as_array()
        .ok_or_else(|| configuration_error("elenco persone non valido nel manifest"))?;
    if patients.len() != 1 {
        return Err(configuration_error(
            "il campione TXT richiede una sola persona",
        ));
    }
    let patient_id = patients[0]["id"]
        .as_str()
        .ok_or_else(|| configuration_error("id persona mancante nel manifest"))?;
    let index = fs::read_to_string(target.join("index.html"))
        .map_err(|cause| filesystem_error("indice principale non leggibile", target, cause))?;
    let expected_href = format!("href=\"patients/{patient_id}/web/index.html\"");
    if !index.contains(&expected_href) {
        return Err(configuration_error(
            "link alla pagina persona mancante dall'indice",
        ));
    }
    validate_patient(target, patient_id)?;

    let checksums = manifest["checksums"]
        .as_object()
        .ok_or_else(|| configuration_error("tabella checksum non valida nel manifest"))?;
    validate_checksums(target, checksums)
}

fn validate_patient(target: &Path, patient_id: &str) -> Result<(), OperationError> {
    let patient_component = safe_relative_path(patient_id)?;
    if patient_component.components().count() != 1 {
        return Err(configuration_error("id persona non valido nel manifest"));
    }
    let patient_root = target.join("patients").join(patient_component);
    for required in [
        patient_root.join("medical_archive.db"),
        patient_root.join("web/index.html"),
        patient_root.join("web/data/documents.json"),
    ] {
        if !required.is_file() {
            return Err(configuration_error(format!(
                "artefatto richiesto mancante: {}",
                required.display()
            )));
        }
    }
    validate_document_links(&patient_root)
}

fn validate_document_links(patient_root: &Path) -> Result<(), OperationError> {
    let data_path = patient_root.join("web/data/documents.json");
    let data: Value =
        serde_json::from_slice(&fs::read(&data_path).map_err(|cause| {
            filesystem_error("dati documenti non leggibili", &data_path, cause)
        })?)
        .map_err(|cause| configuration_error(format!("JSON documenti non valido: {cause}")))?;
    let documents = data["documents"]
        .as_array()
        .ok_or_else(|| configuration_error("elenco documenti non valido"))?;
    let page_path = patient_root.join("web/index.html");
    let page = fs::read_to_string(&page_path)
        .map_err(|cause| filesystem_error("pagina non leggibile", &page_path, cause))?;
    for document in documents {
        let href = document["href"]
            .as_str()
            .ok_or_else(|| configuration_error("href documento mancante"))?;
        if !href.starts_with("../documents/")
            || href.contains('\\')
            || href.contains(':')
            || href.contains('?')
            || href.contains('#')
        {
            return Err(configuration_error(format!(
                "href non relativo non ammesso: {href}"
            )));
        }
        let relative = percent_decode(&href["../documents/".len()..])?;
        let relative = safe_relative_path(&relative)?;
        let linked = patient_root.join("documents").join(relative);
        if !linked.is_file() {
            return Err(configuration_error(format!(
                "documento linkato mancante: {}",
                linked.display()
            )));
        }
        let escaped_href = href.replace('&', "&amp;").replace('"', "&quot;");
        if !page.contains(&format!("href=\"{escaped_href}\"")) {
            return Err(configuration_error(format!(
                "link assente dalla pagina: {href}"
            )));
        }
    }
    Ok(())
}

fn validate_checksums(target: &Path, checksums: &Map<String, Value>) -> Result<(), OperationError> {
    let mut declared = BTreeSet::new();
    for (relative, expected) in checksums {
        let path = safe_relative_path(relative)?;
        let expected_hash = expected
            .as_str()
            .ok_or_else(|| configuration_error("checksum non testuale nel manifest"))?;
        let file = target.join(&path);
        if !file.is_file() || sha256_file(&file)? != expected_hash {
            return Err(configuration_error(format!(
                "checksum mancante o non coincidente: {relative}"
            )));
        }
        declared.insert(relative.replace('\\', "/"));
    }
    let mut actual = BTreeSet::new();
    collect_relative_files(target, target, &mut actual)?;
    actual.remove("SANIKEY-MANIFEST.json");
    if declared != actual {
        return Err(configuration_error("insieme file diverso dal manifest"));
    }
    Ok(())
}

fn write_manifest(
    root: &Path,
    person: &PersonConfig,
    checksums: BTreeMap<String, String>,
) -> Result<(), OperationError> {
    let path = root.join("SANIKEY-MANIFEST.json");
    let manifest = json!({
        "schema_version": 1,
        "exported_at": null,
        "sanikey_version": crate::version(),
        "copy_strategy": "simulated",
        "patients": [{
            "id": person.id,
            "display_name": person.display_name,
            "usb_uuid": person.usb_uuid,
        }],
        "checksums": checksums,
    });
    let contents = serde_json::to_string_pretty(&manifest).map_err(|cause| {
        configuration_error(format!("impossibile serializzare il manifest: {cause}"))
    })?;
    fs::write(&path, format!("{contents}\n"))
        .map_err(|cause| filesystem_error("impossibile scrivere il manifest", &path, cause))
}

fn write_root_index(root: &Path, person: &PersonConfig) -> Result<(), OperationError> {
    let href = format!("patients/{}/web/index.html", person.id);
    let html = format!(
        "<!doctype html>\n<html lang=\"it\"><head><meta charset=\"utf-8\"><title>SaniKey</title></head><body><main><h1>Archivio SaniKey</h1><ul><li><a href=\"{}\">{}</a></li></ul></main></body></html>\n",
        escape_html(&href),
        escape_html(&person.display_name)
    );
    let path = root.join("index.html");
    fs::write(&path, html)
        .map_err(|cause| filesystem_error("impossibile scrivere l'indice USB", &path, cause))
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), OperationError> {
    if !source.is_dir() {
        return Err(configuration_error(format!(
            "directory di build mancante: {}",
            source.display()
        )));
    }
    fs::create_dir_all(target).map_err(|cause| {
        filesystem_error("impossibile creare la directory export", target, cause)
    })?;
    let mut entries = fs::read_dir(source)
        .map_err(|cause| filesystem_error("directory di build non leggibile", source, cause))?
        .map(|entry| {
            entry.map_err(|cause| {
                filesystem_error("elemento della build non leggibile", source, cause)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let kind = entry.file_type().map_err(|cause| {
            filesystem_error("tipo elemento build non leggibile", &source_path, cause)
        })?;
        if kind.is_symlink() {
            return Err(configuration_error(format!(
                "link simbolico non ammesso nell'export: {}",
                source_path.display()
            )));
        }
        if kind.is_dir() {
            copy_tree(&source_path, &target_path)?;
        } else if kind.is_file() {
            copy_file(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn copy_file(source: &Path, target: &Path) -> Result<(), OperationError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|cause| {
            filesystem_error("impossibile creare la directory export", parent, cause)
        })?;
    }
    fs::copy(source, target)
        .map_err(|cause| filesystem_error("impossibile copiare l'artefatto", target, cause))?;
    Ok(())
}

fn checksum_tree(root: &Path) -> Result<BTreeMap<String, String>, OperationError> {
    let mut files = BTreeSet::new();
    collect_relative_files(root, root, &mut files)?;
    files
        .into_iter()
        .map(|relative| {
            let path = root.join(&relative);
            Ok((relative, sha256_file(&path)?))
        })
        .collect()
}

fn collect_relative_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), OperationError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|cause| filesystem_error("directory export non leggibile", directory, cause))?
        .map(|entry| {
            entry.map_err(|cause| {
                filesystem_error("elemento export non leggibile", directory, cause)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().map_err(|cause| {
            filesystem_error("tipo elemento export non leggibile", &path, cause)
        })?;
        if kind.is_symlink() {
            return Err(configuration_error(format!(
                "link simbolico non ammesso nell'export: {}",
                path.display()
            )));
        }
        if kind.is_dir() {
            collect_relative_files(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path.strip_prefix(root).map_err(|cause| {
                configuration_error(format!("percorso export fuori dalla radice: {cause}"))
            })?;
            let normalized = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.insert(normalized);
        }
    }
    Ok(())
}

fn safe_relative_path(value: &str) -> Result<PathBuf, OperationError> {
    if value.is_empty() || value.contains('\\') || value.starts_with('/') {
        return Err(configuration_error(format!(
            "percorso export non relativo: {value}"
        )));
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(configuration_error(format!(
            "percorso export non sicuro: {value}"
        )));
    }
    Ok(path.to_path_buf())
}

fn percent_decode(value: &str) -> Result<String, OperationError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let end = index + 3;
            if end > bytes.len() {
                return Err(configuration_error("encoding percent incompleto nel link"));
            }
            let digits = std::str::from_utf8(&bytes[index + 1..end])
                .map_err(|_| configuration_error("encoding percent non valido nel link"))?;
            let byte = u8::from_str_radix(digits, 16)
                .map_err(|_| configuration_error("encoding percent non valido nel link"))?;
            decoded.push(byte);
            index = end;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| configuration_error("percorso UTF-8 non valido nel link"))
}

fn sha256_file(path: &Path) -> Result<String, OperationError> {
    let mut file = fs::File::open(path)
        .map_err(|cause| filesystem_error("file export non leggibile", path, cause))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let length = file
            .read(&mut buffer)
            .map_err(|cause| filesystem_error("impossibile calcolare SHA-256", path, cause))?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn count_files(root: &Path) -> Result<usize, OperationError> {
    let mut files = BTreeSet::new();
    collect_relative_files(root, root, &mut files)?;
    Ok(files.len())
}

fn absolute_path(path: &Path) -> Result<PathBuf, OperationError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|cause| {
                configuration_error(format!("directory corrente non disponibile: {cause}"))
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

fn reject_input_overlap(person: &PersonConfig, target: &Path) -> Result<(), OperationError> {
    for input in [&person.source_documents, &person.local_build] {
        let input = absolute_path(input)?;
        if target.starts_with(&input) || input.starts_with(target) {
            return Err(configuration_error(format!(
                "destinazione simulata sovrapposta agli input: {}",
                target.display()
            )));
        }
    }
    Ok(())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn filesystem_error(context: &str, path: &Path, cause: std::io::Error) -> OperationError {
    configuration_error(format!("{context} {}: {cause}", path.display()))
}

fn configuration_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}
