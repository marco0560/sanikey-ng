//! Estrazione confinata e inventario dei membri di archivi e immagini disco.

#[cfg(test)]
mod tests;

use crate::config::{IngestionConfig, PersonConfig};
use crate::inventory::{is_ingestion_excluded, staged_document_record};
use crate::logging::{
    Component as LogComponent, TraceField, TraceOmission, TraceValue, trace_current,
    trace_sensitive_text,
};
use crate::metadata::DocumentRecord;
use crate::operation::{ErrorCode, OperationError};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, UNIX_EPOCH};

const CONTAINER_SUFFIXES: &[&str] = &[".7z", ".img", ".iso", ".rar", ".tar.xz", ".zip"];

macro_rules! trace_container_call {
    ($name:literal, [$($field:expr),* $(,)?], result $result_projection:expr, $body:expr) => {{
        let trace = trace_current(LogComponent::Core, $name, || vec![$($field),*]);
        let result = run_container_body(|| $body);
        if let Some(trace) = trace {
            let _ = trace.finish(($result_projection)(&result));
        }
        result
    }};
    ($name:literal, [$($field:expr),* $(,)?], $body:expr) => {{
        let trace = trace_current(LogComponent::Core, $name, || vec![$($field),*]);
        let result = run_container_body(|| $body);
        if let Some(trace) = trace {
            let _ = trace.finish(result.trace_value());
        }
        result
    }};
}

/// Converte gli esiti dei helper contenitori nella forma registrabile in Trace.
trait ContainerTraceOutput {
    fn trace_value(&self) -> TraceValue;
}

/// Registra il valore o l'errore con redazione del testo sensibile.
impl<T: std::fmt::Debug> ContainerTraceOutput for Result<T, OperationError> {
    fn trace_value(&self) -> TraceValue {
        match self {
            Ok(value) => trace_sensitive_text(&format!("{value:#?}")),
            Err(error) => TraceValue::Data(json!({
                "state":"error",
                "code":error.code().as_str(),
                "message":trace_sensitive_text(error.message()).to_json(),
            })),
        }
    }
}

/// Rappresenta l'esito di un'operazione senza valore di ritorno.
impl ContainerTraceOutput for Result<(), String> {
    fn trace_value(&self) -> TraceValue {
        match self {
            Ok(()) => TraceValue::data("succeeded"),
            Err(error) => TraceValue::Data(
                json!({"state":"error","message":trace_sensitive_text(error).to_json()}),
            ),
        }
    }
}

/// Registra un percorso di risultato senza perderne la codifica nativa.
impl ContainerTraceOutput for Result<PathBuf, String> {
    fn trace_value(&self) -> TraceValue {
        match self {
            Ok(path) => TraceValue::path(path),
            Err(error) => TraceValue::Data(
                json!({"state":"error","message":trace_sensitive_text(error).to_json()}),
            ),
        }
    }
}

/// Registra l'esito senza serializzare l'handle del file.
impl ContainerTraceOutput for Result<File, String> {
    fn trace_value(&self) -> TraceValue {
        match self {
            Ok(_) => TraceValue::record(&[
                TraceField::new("state", TraceValue::data("succeeded")),
                TraceField::new(
                    "file_handle",
                    TraceValue::Omitted(TraceOmission::UnsupportedType),
                ),
            ]),
            Err(error) => TraceValue::record(&[
                TraceField::new("state", TraceValue::data("error")),
                TraceField::new("message", trace_sensitive_text(error)),
            ]),
        }
    }
}

/// Registra l'elenco dei percorsi o l'errore di I/O.
impl ContainerTraceOutput for io::Result<Vec<PathBuf>> {
    fn trace_value(&self) -> TraceValue {
        match self {
            Ok(paths) => TraceValue::Data(Value::Array(
                paths
                    .iter()
                    .map(|path| TraceValue::path(path).to_json())
                    .collect(),
            )),
            Err(error) => TraceValue::record(&[
                TraceField::new("state", TraceValue::data("error")),
                TraceField::new("message", trace_sensitive_text(&error.to_string())),
            ]),
        }
    }
}

/// Registra il completamento o l'errore di un'operazione di I/O.
impl ContainerTraceOutput for io::Result<()> {
    fn trace_value(&self) -> TraceValue {
        match self {
            Ok(()) => TraceValue::data("succeeded"),
            Err(error) => TraceValue::record(&[
                TraceField::new("state", TraceValue::data("error")),
                TraceField::new("message", trace_sensitive_text(&error.to_string())),
            ]),
        }
    }
}

/// Redige le assegnazioni di segreti nel testo restituito.
impl ContainerTraceOutput for String {
    fn trace_value(&self) -> TraceValue {
        trace_sensitive_text(self)
    }
}

/// Registra la presenza e il percorso nativo facoltativo.
impl ContainerTraceOutput for Option<PathBuf> {
    fn trace_value(&self) -> TraceValue {
        self.as_deref()
            .map_or_else(|| TraceValue::data(false), TraceValue::path)
    }
}

/// Registra codice e messaggio redatto dell'errore operativo.
impl ContainerTraceOutput for OperationError {
    fn trace_value(&self) -> TraceValue {
        TraceValue::record(&[
            TraceField::new("state", TraceValue::data("error")),
            TraceField::new("code", TraceValue::data(self.code().as_str())),
            TraceField::new("message", trace_sensitive_text(self.message())),
        ])
    }
}

/// Redige la rappresentazione diagnostica dell'errore sevenz.
impl ContainerTraceOutput for sevenz_rust2::Error {
    fn trace_value(&self) -> TraceValue {
        trace_sensitive_text(&format!("{self:?}"))
    }
}

/// Registra direttamente il risultato booleano.
impl ContainerTraceOutput for bool {
    fn trace_value(&self) -> TraceValue {
        TraceValue::data(*self)
    }
}

/// Redige e registra ciascuna stringa del risultato.
impl ContainerTraceOutput for Vec<String> {
    fn trace_value(&self) -> TraceValue {
        TraceValue::Data(Value::Array(
            self.iter()
                .map(|value| trace_sensitive_text(value).to_json())
                .collect(),
        ))
    }
}

fn run_container_body<T>(body: impl FnOnce() -> T) -> T {
    body()
}

fn trace_document(document: &DocumentRecord) -> TraceValue {
    TraceValue::Data(json!({
        "document_id": trace_sensitive_text(&document.document_id).to_json(),
        "patient_id": trace_sensitive_text(&document.patient_id).to_json(),
        "path": TraceValue::path(&document.path).to_json(),
        "title": trace_sensitive_text(&document.title).to_json(),
        "category": trace_sensitive_text(&document.category).to_json(),
        "kind": trace_sensitive_text(&document.kind).to_json(),
        "sha256": trace_sensitive_text(&document.sha256).to_json(),
        "date": document.date.as_deref().map(trace_sensitive_text).map(|value| value.to_json()),
        "series": document.series.as_deref().map(trace_sensitive_text).map(|value| value.to_json()),
        "tags": document.tags.iter().map(|value| trace_sensitive_text(value).to_json()).collect::<Vec<_>>(),
        "origin": trace_sensitive_text(&document.origin).to_json(),
        "container_id": document.container_id.as_deref().map(trace_sensitive_text).map(|value| value.to_json()),
        "internal_path": document.internal_path.as_deref().map(trace_sensitive_text).map(|value| value.to_json()),
    }))
}

fn trace_documents(documents: &[DocumentRecord]) -> TraceValue {
    TraceValue::Data(Value::Array(
        documents
            .iter()
            .map(|document| trace_document(document).to_json())
            .collect(),
    ))
}

fn trace_staging_result(result: &Result<ContainerStagingResult, OperationError>) -> TraceValue {
    match result {
        Ok(result) => TraceValue::record(&[
            TraceField::new("state", TraceValue::data("succeeded")),
            TraceField::new("documents", trace_documents(&result.documents)),
            TraceField::new(
                "nested_containers",
                trace_documents(&result.nested_containers),
            ),
            TraceField::new(
                "members_debug",
                trace_sensitive_text(&format!("{:#?}", result.members)),
            ),
            TraceField::new(
                "member_path_native_encoding",
                TraceValue::Omitted(TraceOmission::UnsupportedType),
            ),
            TraceField::new("manifest", TraceValue::path(&result.manifest)),
            TraceField::new(
                "warning_messages",
                TraceValue::Data(Value::Array(
                    result
                        .warning_messages
                        .iter()
                        .map(|warning| trace_sensitive_text(warning).to_json())
                        .collect(),
                )),
            ),
        ]),
        Err(error) => error.trace_value(),
    }
}

fn trace_member_result(
    result: &Result<StagedContainerMember, OperationError>,
    source_path: &Path,
) -> TraceValue {
    match result {
        Ok(result) => TraceValue::record(&[
            TraceField::new("state", TraceValue::data("succeeded")),
            TraceField::new("value_debug", trace_sensitive_text(&format!("{result:#?}"))),
            TraceField::new("source_path_native", TraceValue::path(source_path)),
            TraceField::new(
                "output_path_encoding",
                TraceValue::data("lossy_utf8_string_in_StagedContainerMember"),
            ),
        ]),
        Err(error) => error.trace_value(),
    }
}

/// Rappresenta un file derivato da un membro estratto.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedContainerMember {
    /// Identificatore SHA-256 del documento contenitore.
    pub container_id: String,
    /// Identificatore SHA-256 del file estratto.
    pub document_id: String,
    /// Percorso POSIX del membro dentro il contenitore.
    pub internal_path: String,
    /// Percorso assoluto del file generato nello staging.
    pub path: String,
    /// Tipo documentale classificato dal nome e dal contenuto.
    pub kind: String,
    /// Digest SHA-256 dei byte estratti.
    pub sha256: String,
    /// Dimensione del file in byte.
    pub size: u64,
    /// Tempo di modifica espresso in nanosecondi da Unix epoch.
    pub mtime_ns: u128,
}

/// Risultati dell'estrazione ricorsiva dei contenitori.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerStagingResult {
    /// Membri ammessi nella pipeline documentale.
    pub documents: Vec<DocumentRecord>,
    /// Manifest di tutti i file regolari estratti.
    pub members: Vec<StagedContainerMember>,
    /// Errori di singoli contenitori che non interrompono lo staging.
    pub warning_messages: Vec<String>,
    /// Percorso del manifest JSON generato.
    pub manifest: PathBuf,
    /// Percorsi di archivio derivati da membri estratti.
    pub nested_containers: Vec<DocumentRecord>,
}

/// Estrae i contenitori riconosciuti e scrive il manifest di provenienza.
///
/// I membri vengono scritti solo sotto `local_build/staging/containers`.
/// Il riconoscimento mantiene le estensioni della baseline; ogni parser
/// verifica il contenuto del formato prima di estrarre.
///
/// # Errors
///
/// Restituisce `ErrorCode::Configuration` se non è possibile creare o
/// sostituire le directory di staging, acquisire il lock o scrivere il
/// manifest.
pub fn stage_container_documents(
    person: &PersonConfig,
    documents: &[DocumentRecord],
) -> Result<ContainerStagingResult, OperationError> {
    trace_container_call!(
        "containers::stage_container_documents",
        [
            TraceField::new("person_id", trace_sensitive_text(&person.id)),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
            TraceField::new("documents", trace_documents(documents)),
            TraceField::new(
                "archive_payload",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        result trace_staging_result,
        stage_container_documents_impl(person, documents)
    )
}

fn stage_container_documents_impl(
    person: &PersonConfig,
    documents: &[DocumentRecord],
) -> Result<ContainerStagingResult, OperationError> {
    trace_container_call!(
        "containers::stage_container_documents_impl",
        [
            TraceField::new("person_id", trace_sensitive_text(&person.id)),
            TraceField::new("local_build", TraceValue::path(&person.local_build)),
            TraceField::new("documents", trace_documents(documents)),
            TraceField::new(
                "lock_and_staging_state",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        result trace_staging_result,
        {
            fs::create_dir_all(&person.local_build).map_err(|cause| {
                filesystem_error(
                    "impossibile creare la directory di build",
                    &person.local_build,
                    cause,
                )
            })?;
            let lock_path = person.local_build.join(".container-staging.lock");
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&lock_path)
                .map_err(|cause| {
                    filesystem_error("impossibile aprire il lock", &lock_path, cause)
                })?;
            lock.lock().map_err(|cause| {
                filesystem_error("impossibile acquisire il lock", &lock_path, cause)
            })?;

            let staging_root = person.local_build.join("staging").join("containers");
            match fs::remove_dir_all(&staging_root) {
                Ok(()) => {}
                Err(cause) if cause.kind() == io::ErrorKind::NotFound => {}
                Err(cause) => {
                    return Err(filesystem_error(
                        "impossibile sostituire lo staging dei contenitori",
                        &staging_root,
                        cause,
                    ));
                }
            }
            fs::create_dir_all(&staging_root).map_err(|cause| {
                filesystem_error(
                    "impossibile creare lo staging dei contenitori",
                    &staging_root,
                    cause,
                )
            })?;

            let mut initial: Vec<_> = documents
                .iter()
                .filter(|document| is_container_path(&document.path))
                .cloned()
                .collect();
            initial.sort_by(|left, right| left.path.cmp(&right.path));
            let mut queue: VecDeque<_> = initial.into();
            let mut processed = HashSet::new();
            let mut result = ContainerStagingResult {
                documents: Vec::new(),
                members: Vec::new(),
                warning_messages: Vec::new(),
                manifest: person
                    .local_build
                    .join("manifests")
                    .join("container_staging.json"),
                nested_containers: Vec::new(),
            };

            while let Some(container) = queue.pop_front() {
                if !processed.insert(container.document_id.clone()) {
                    continue;
                }
                let target = staging_root.join(&container.document_id);
                fs::create_dir_all(&target).map_err(|cause| {
                    filesystem_error(
                        "impossibile creare lo staging del contenitore",
                        &target,
                        cause,
                    )
                })?;
                if let Err(cause) = extract_container(&container.path, &target) {
                    let _ = fs::remove_dir_all(&target);
                    result.warning_messages.push(format!(
                        "{}: staging contenitore non riuscito: {cause}",
                        container.path.display()
                    ));
                    continue;
                }
                let mut paths = regular_files(&target).map_err(|cause| {
                    filesystem_error(
                        "impossibile leggere lo staging del contenitore",
                        &target,
                        cause,
                    )
                })?;
                paths.sort();
                for path in paths {
                    let document =
                        staged_document_record(path, &target, &person.id, &container.document_id)?;
                    result.members.push(member_record(&container, &document)?);
                    if should_ingest_staged_document(&person.ingestion, &document) {
                        result.documents.push(document.clone());
                    }
                    if is_container_path(&document.path) {
                        result.nested_containers.push(document.clone());
                        queue.push_back(document);
                    }
                }
            }

            write_manifest(person, &mut result)?;
            let _ = lock.unlock();
            Ok(result)
        }
    )
}

fn is_container_path(path: &Path) -> bool {
    trace_container_call!(
        "containers::is_container_path",
        [TraceField::new("path", TraceValue::path(path))],
        is_container_path_impl(path)
    )
}

fn is_container_path_impl(path: &Path) -> bool {
    trace_container_call!(
        "containers::is_container_path_impl",
        [TraceField::new("path", TraceValue::path(path))],
        {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let name = name.to_lowercase();
            CONTAINER_SUFFIXES
                .iter()
                .any(|suffix| name.ends_with(suffix))
        }
    )
}

fn should_ingest_staged_document(ingestion: &IngestionConfig, document: &DocumentRecord) -> bool {
    trace_container_call!(
        "containers::should_ingest_staged_document",
        [
            TraceField::new(
                "ingestion",
                trace_sensitive_text(&format!("{ingestion:#?}"))
            ),
            TraceField::new("document", trace_document(document)),
        ],
        should_ingest_staged_document_impl(ingestion, document)
    )
}

fn should_ingest_staged_document_impl(
    ingestion: &IngestionConfig,
    document: &DocumentRecord,
) -> bool {
    trace_container_call!(
        "containers::should_ingest_staged_document_impl",
        [
            TraceField::new(
                "ingestion",
                trace_sensitive_text(&format!("{ingestion:#?}"))
            ),
            TraceField::new("document", trace_document(document)),
        ],
        {
            if document
                .internal_path
                .as_deref()
                .is_some_and(|path| is_ingestion_excluded(ingestion, path))
            {
                return false;
            }
            matches!(
                document.kind.as_str(),
                "dicom_file" | "dicom_img" | "dicom_iso" | "office" | "pdf" | "text"
            )
        }
    )
}

fn extract_container(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_container",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "archive_payload",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        extract_container_impl(source, target)
    )
}

fn extract_container_impl(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_container_impl",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "archive_payload",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        {
            let suffix = known_suffix(source);
            match suffix.as_str() {
                ".zip" => extract_zip(source, target),
                ".7z" => extract_7z(source, target),
                ".tar.xz" => extract_tar_xz(source, target),
                ".rar" => extract_rar(source, target),
                ".img" | ".iso" => extract_disk_image(source, target),
                _ => Err(format!("formato contenitore non supportato {suffix}")),
            }
        }
    )
}

fn extract_zip(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_zip",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        extract_zip_impl(source, target)
    )
}

fn extract_zip_impl(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_zip_impl",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            )
        ],
        {
            let file = File::open(source).map_err(|cause| cause.to_string())?;
            let mut archive = zip::ZipArchive::new(file).map_err(|cause| cause.to_string())?;
            for index in 0..archive.len() {
                let mut member = archive.by_index(index).map_err(|cause| cause.to_string())?;
                if member.is_dir() {
                    continue;
                }
                let relative = member.enclosed_name().ok_or_else(|| {
                    format!("percorso membro contenitore non sicuro: {}", member.name())
                })?;
                let output = checked_output_path(target, &relative)?;
                write_member(&mut member, &output)?;
            }
            Ok(())
        }
    )
}

fn extract_7z(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_7z",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new(
                "archive_password",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        extract_7z_impl(source, target)
    )
}

fn extract_7z_impl(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_7z_impl",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new(
                "archive_password",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            )
        ],
        {
            let result = sevenz_rust2::decompress_file_with_extract_fn(
                source,
                target,
                |entry, reader, destination| {
                    if entry.is_directory() || entry.is_anti_item() {
                        return Ok(true);
                    }
                    let safe_path = safe_member_path(target, entry.name()).map_err(sevenz_error)?;
                    if safe_path != *destination {
                        return Err(sevenz_error("percorso membro 7z non confinato"));
                    }
                    let mut output = create_member(&safe_path).map_err(sevenz_error)?;
                    io::copy(reader, &mut output)
                        .map_err(|cause| sevenz_rust2::Error::Io(cause, "read member".into()))?;
                    output
                        .flush()
                        .map_err(|cause| sevenz_rust2::Error::Io(cause, "flush member".into()))?;
                    Ok(true)
                },
            );
            let Err(rust_error) = result else {
                return Ok(());
            };
            clear_directory_contents(target).map_err(|cause| cause.to_string())?;
            let seven_zip = find_program("7zz")
                .or_else(|| find_program("7z"))
                .ok_or_else(|| format!("estrazione 7z Rust non riuscita: {rust_error}"))?;
            let listing = Command::new(&seven_zip)
                .args(["l", "-slt", "-p-"])
                .arg(source)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .map_err(|cause| cause.to_string())?;
            if !listing.status.success() {
                return Err(format!("estrazione 7z Rust non riuscita: {rust_error}"));
            }
            let names = parse_7z_listing(&String::from_utf8_lossy(&listing.stdout));
            extract_listed_members(&seven_zip, source, target, &names, true, None)
        }
    )
}

fn extract_tar_xz(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_tar_xz",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        extract_tar_xz_impl(source, target)
    )
}

fn extract_tar_xz_impl(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_tar_xz_impl",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            )
        ],
        {
            let input = File::open(source).map_err(|cause| cause.to_string())?;
            let decoder = xz2::read::XzDecoder::new_multi_decoder(input);
            let mut archive = tar::Archive::new(decoder);
            let entries = archive.entries().map_err(|cause| cause.to_string())?;
            for entry in entries {
                let mut entry = entry.map_err(|cause| cause.to_string())?;
                if !entry.header().entry_type().is_file() {
                    continue;
                }
                let archive_path = entry.path().map_err(|cause| cause.to_string())?;
                let path_trace =
                    trace_current(LogComponent::Core, "containers::tar_member_path", || {
                        vec![TraceField::new(
                            "archive_path_native",
                            TraceValue::path(archive_path.as_ref()),
                        )]
                    });
                let name = archive_path.to_string_lossy().replace('\\', "/");
                if let Some(path_trace) = path_trace {
                    let _ = path_trace.finish(TraceValue::data(json!({
                        "normalized_name_lossy": name,
                        "native_path_conversion": "to_string_lossy",
                    })));
                }
                let relative = safe_member_path(target, &name)?;
                let output = create_member(&relative)?;
                let mut output = output;
                io::copy(&mut entry, &mut output).map_err(|cause| cause.to_string())?;
            }
            Ok(())
        }
    )
}

fn extract_rar(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_rar",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new(
                "archive_password",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        extract_rar_impl(source, target)
    )
}

fn extract_rar_impl(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_rar_impl",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new(
                "archive_password",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            )
        ],
        {
            let unrar =
                find_program("unrar").ok_or_else(|| "comando unrar non installato".to_owned())?;
            let validation = Command::new(&unrar)
                .args(["t", "-p-"])
                .arg(source)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|cause| cause.to_string())?;
            if !validation.success() {
                return Err("verifica del contenitore RAR non riuscita".to_owned());
            }
            let listing = Command::new(&unrar)
                .args(["lb", "-p-"])
                .arg(source)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .map_err(|cause| cause.to_string())?;
            if !listing.status.success() {
                return Err("elenco RAR non riuscito".to_owned());
            }
            let names = String::from_utf8_lossy(&listing.stdout);
            for name in names
                .lines()
                .filter(|name| !name.is_empty() && !name.ends_with('/') && !name.ends_with('\\'))
            {
                let normalized = name.trim_end_matches(['\r', '/']);
                let relative = safe_member_path(target, normalized)?;
                let output = create_member(&relative)?;
                extract_process_to_file(
                    Command::new(&unrar)
                        .args(["p", "-inul", "-idq", "-p-"])
                        .arg(source)
                        .arg(name),
                    output,
                    None,
                )?;
            }
            Ok(())
        }
    )
}

fn extract_disk_image(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_disk_image",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new(
                "archive_password",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        extract_disk_image_impl(source, target)
    )
}

fn extract_disk_image_impl(source: &Path, target: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_disk_image_impl",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new(
                "archive_password",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            )
        ],
        {
            let seven_zip = find_program("7zz").or_else(|| find_program("7z"));
            if let Some(seven_zip) = seven_zip {
                let listing = Command::new(&seven_zip)
                    .args(["l", "-slt", "-p-"])
                    .arg(source)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .map_err(|cause| cause.to_string())?;
                if listing.status.success() {
                    let names = parse_7z_listing(&String::from_utf8_lossy(&listing.stdout));
                    if extract_listed_members(
                        &seven_zip,
                        source,
                        target,
                        &names,
                        true,
                        Some(Duration::from_secs(300)),
                    )
                    .is_ok()
                    {
                        return Ok(());
                    }
                    clear_directory_contents(target).map_err(|cause| cause.to_string())?;
                }
            }

            let bsdtar = find_program("bsdtar").ok_or_else(|| {
                "comandi 7z e bsdtar non disponibili o immagine non leggibile".to_owned()
            })?;
            let listing = Command::new(&bsdtar)
                .args(["-tf"])
                .arg(source)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
                .map_err(|cause| cause.to_string())?;
            if !listing.status.success() {
                return Err("elenco dell'immagine con bsdtar non riuscito".to_owned());
            }
            let names: Vec<_> = String::from_utf8_lossy(&listing.stdout)
                .lines()
                .map(str::to_owned)
                .collect();
            extract_listed_members(
                &bsdtar,
                source,
                target,
                &names,
                false,
                Some(Duration::from_secs(300)),
            )
        }
    )
}

fn extract_listed_members(
    program: &Path,
    source: &Path,
    target: &Path,
    names: &[String],
    seven_zip: bool,
    total_timeout: Option<Duration>,
) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_listed_members",
        [
            TraceField::new("program", TraceValue::path(program)),
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_names",
                TraceValue::Data(Value::Array(
                    names
                        .iter()
                        .map(|name| trace_sensitive_text(name).to_json())
                        .collect(),
                )),
            ),
            TraceField::new("seven_zip_mode", TraceValue::data(seven_zip)),
            TraceField::new(
                "total_timeout",
                TraceValue::data(format!("{total_timeout:?}"))
            ),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
        ],
        extract_listed_members_impl(program, source, target, names, seven_zip, total_timeout)
    )
}

fn extract_listed_members_impl(
    program: &Path,
    source: &Path,
    target: &Path,
    names: &[String],
    seven_zip: bool,
    total_timeout: Option<Duration>,
) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_listed_members_impl",
        [
            TraceField::new("program", TraceValue::path(program)),
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "member_names",
                TraceValue::Data(Value::Array(
                    names
                        .iter()
                        .map(|name| trace_sensitive_text(name).to_json())
                        .collect()
                ))
            ),
            TraceField::new("seven_zip_mode", TraceValue::data(seven_zip)),
            TraceField::new(
                "total_timeout",
                TraceValue::data(format!("{total_timeout:?}"))
            ),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            )
        ],
        {
            let started = std::time::Instant::now();
            for name in names
                .iter()
                .filter(|name| !name.is_empty() && !name.ends_with('/') && !name.ends_with('\\'))
            {
                let normalized = name.trim_end_matches(['\r', '/']);
                if normalized.is_empty() {
                    continue;
                }
                let relative = safe_member_path(target, normalized)?;
                let output = create_member(&relative)?;
                let mut command = Command::new(program);
                if seven_zip {
                    command
                        .args(["x", "-so", "-y", "-spd", "-p-"])
                        .arg(source)
                        .arg(name);
                } else {
                    command.args(["-xOf"]).arg(source).arg(name);
                }
                let timeout = total_timeout.map(|limit| limit.saturating_sub(started.elapsed()));
                extract_process_to_file(&mut command, output, timeout)?;
            }
            Ok(())
        }
    )
}

fn parse_7z_listing(listing: &str) -> Vec<String> {
    trace_container_call!(
        "containers::parse_7z_listing",
        [
            TraceField::new("listing", trace_sensitive_text(listing)),
            TraceField::new("encoding", TraceValue::data("utf8_lossy_upstream")),
        ],
        parse_7z_listing_impl(listing)
    )
}

fn parse_7z_listing_impl(listing: &str) -> Vec<String> {
    trace_container_call!(
        "containers::parse_7z_listing_impl",
        [TraceField::new("listing", trace_sensitive_text(listing))],
        {
            let mut in_members = false;
            let mut name = None;
            let mut directory = false;
            let mut result = Vec::new();
            for line in listing.lines().chain(std::iter::once("----------")) {
                if line.starts_with("----------") {
                    if in_members
                        && !directory
                        && let Some(name) = name.take().filter(|name: &String| !name.is_empty())
                    {
                        result.push(name);
                    }
                    in_members = true;
                    name = None;
                    directory = false;
                } else if in_members {
                    if let Some(value) = line.strip_prefix("Path = ") {
                        name = Some(value.to_owned());
                    } else if line == "Folder = +" {
                        directory = true;
                    }
                }
            }
            result
        }
    )
}

fn extract_process_to_file(
    command: &mut Command,
    output: File,
    timeout: Option<Duration>,
) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_process_to_file",
        [
            TraceField::new("command", trace_sensitive_text(&format!("{command:?}"))),
            TraceField::new(
                "output",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new("timeout", TraceValue::data(format!("{timeout:?}"))),
        ],
        extract_process_to_file_impl(command, output, timeout)
    )
}

fn extract_process_to_file_impl(
    command: &mut Command,
    mut output: File,
    timeout: Option<Duration>,
) -> Result<(), String> {
    trace_container_call!(
        "containers::extract_process_to_file_impl",
        [
            TraceField::new("command", trace_sensitive_text(&format!("{command:?}"))),
            TraceField::new(
                "output",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            ),
            TraceField::new("timeout", TraceValue::data(format!("{timeout:?}")))
        ],
        {
            let mut child = command
                .stdout(Stdio::from(
                    output.try_clone().map_err(|cause| cause.to_string())?,
                ))
                .stderr(Stdio::null())
                .spawn()
                .map_err(|cause| cause.to_string())?;
            let started = std::time::Instant::now();
            loop {
                match child.try_wait().map_err(|cause| cause.to_string())? {
                    Some(status) if status.success() => {
                        output.flush().map_err(|cause| cause.to_string())?;
                        return Ok(());
                    }
                    Some(_) => {
                        return Err("estrazione del membro non riuscita".to_owned());
                    }
                    None if timeout.is_some_and(|limit| started.elapsed() >= limit) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err("timeout durante l'estrazione del membro".to_owned());
                    }
                    None => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        }
    )
}

fn safe_member_path(target: &Path, name: &str) -> Result<PathBuf, String> {
    trace_container_call!(
        "containers::safe_member_path",
        [
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new("member_name", trace_sensitive_text(name)),
        ],
        safe_member_path_impl(target, name)
    )
}

fn safe_member_path_impl(target: &Path, name: &str) -> Result<PathBuf, String> {
    trace_container_call!(
        "containers::safe_member_path_impl",
        [
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new("member_name", trace_sensitive_text(name))
        ],
        {
            let normalized = name.replace('\\', "/");
            let first = normalized.split('/').next().unwrap_or_default();
            let has_drive_prefix = first.as_bytes().get(1) == Some(&b':');
            let path = Path::new(&normalized);
            if normalized.starts_with('/')
                || has_drive_prefix
                || path.components().any(|part| part == Component::ParentDir)
                || (cfg!(windows) && windows_member_has_device_name(path))
            {
                return Err(format!("percorso membro contenitore non sicuro: {name}"));
            }
            let relative = path
                .components()
                .filter(|part| *part != Component::CurDir)
                .collect::<PathBuf>();
            checked_output_path(target, &relative)
        }
    )
}

fn windows_member_has_device_name(path: &Path) -> bool {
    trace_container_call!(
        "containers::windows_member_has_device_name",
        [TraceField::new("path", TraceValue::path(path))],
        path.components().any(|component| {
            let Component::Normal(component) = component else {
                return false;
            };
            let name = component.to_string_lossy();
            if name.contains(':') || name.ends_with(['.', ' ']) {
                return true;
            }
            let stem = name
                .split('.')
                .next()
                .unwrap_or_default()
                .trim_end_matches(['.', ' '])
                .to_ascii_uppercase();
            matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ["COM", "LPT"].iter().any(|prefix| {
                    stem.strip_prefix(prefix).is_some_and(|suffix| {
                        matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                    })
                })
        })
    )
}

fn checked_output_path(target: &Path, relative: &Path) -> Result<PathBuf, String> {
    trace_container_call!(
        "containers::checked_output_path",
        [
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new("relative", TraceValue::path(relative))
        ],
        {
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|part| part == Component::ParentDir)
            {
                return Err(format!(
                    "percorso membro contenitore non sicuro: {}",
                    relative.display()
                ));
            }
            Ok(target.join(relative))
        }
    )
}

fn write_member(input: &mut impl Read, path: &Path) -> Result<(), String> {
    trace_container_call!(
        "containers::write_member",
        [
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new(
                "member_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent)
            )
        ],
        {
            let mut output = create_member(path)?;
            io::copy(input, &mut output).map_err(|cause| cause.to_string())?;
            Ok(())
        }
    )
}

fn create_member(path: &Path) -> Result<File, String> {
    trace_container_call!(
        "containers::create_member",
        [TraceField::new("path", TraceValue::path(path))],
        {
            let parent = path
                .parent()
                .ok_or_else(|| "percorso membro senza directory padre".to_owned())?;
            fs::create_dir_all(parent).map_err(|cause| cause.to_string())?;
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)
                .map_err(|cause| cause.to_string())
        }
    )
}

fn regular_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    trace_container_call!(
        "containers::regular_files",
        [TraceField::new("root", TraceValue::path(root))],
        {
            let mut files = Vec::new();
            let mut pending = vec![root.to_path_buf()];
            while let Some(directory) = pending.pop() {
                for entry in fs::read_dir(directory)? {
                    let entry = entry?;
                    let file_type = entry.file_type()?;
                    if file_type.is_dir() {
                        pending.push(entry.path());
                    } else if file_type.is_file() {
                        files.push(entry.path());
                    }
                }
            }
            Ok(files)
        }
    )
}

fn clear_directory_contents(path: &Path) -> io::Result<()> {
    trace_container_call!(
        "containers::clear_directory_contents",
        [TraceField::new("path", TraceValue::path(path))],
        {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let item_path = entry.path();
                if entry.file_type()?.is_dir() {
                    fs::remove_dir_all(item_path)?;
                } else {
                    fs::remove_file(item_path)?;
                }
            }
            Ok(())
        }
    )
}

fn member_record(
    container: &DocumentRecord,
    document: &DocumentRecord,
) -> Result<StagedContainerMember, OperationError> {
    trace_container_call!(
        "containers::member_record",
        [
            TraceField::new("container", trace_document(container)),
            TraceField::new("document", trace_document(document))
        ],
        result |result: &Result<StagedContainerMember, OperationError>| {
            trace_member_result(result, &document.path)
        },
        {
            let metadata = document.path.metadata().map_err(|cause| {
                filesystem_error(
                    "impossibile leggere i metadati del membro",
                    &document.path,
                    cause,
                )
            })?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos());
            Ok(StagedContainerMember {
                container_id: container.document_id.clone(),
                document_id: document.document_id.clone(),
                internal_path: document.internal_path.clone().unwrap_or_default(),
                path: document.path.to_string_lossy().into_owned(),
                kind: document.kind.clone(),
                sha256: document.sha256.clone(),
                size: metadata.len(),
                mtime_ns: modified,
            })
        }
    )
}

fn write_manifest(
    person: &PersonConfig,
    result: &mut ContainerStagingResult,
) -> Result<(), OperationError> {
    trace_container_call!(
        "containers::write_manifest",
        [
            TraceField::new("person_id", trace_sensitive_text(&person.id)),
            TraceField::new("manifest", TraceValue::path(&result.manifest)),
            TraceField::new(
                "members",
                trace_sensitive_text(&format!("{:#?}", result.members))
            ),
            TraceField::new(
                "warnings",
                TraceValue::Data(Value::Array(
                    result
                        .warning_messages
                        .iter()
                        .map(|warning| trace_sensitive_text(warning).to_json())
                        .collect()
                ))
            )
        ],
        {
            let members: Vec<Value> = result
                .members
                .iter()
                .map(|member| {
                    json!({
                        "container_id": member.container_id,
                        "document_id": member.document_id,
                        "internal_path": member.internal_path,
                        "path": member.path,
                        "kind": member.kind,
                        "sha256": member.sha256,
                        "size": member.size,
                        "mtime_ns": member.mtime_ns,
                    })
                })
                .collect();
            let payload = json!({
                "schema_version": 1,
                "patient_id": person.id,
                "members": members,
                "warnings": result.warning_messages,
            });
            let parent = result
                .manifest
                .parent()
                .expect("manifest sotto local_build");
            fs::create_dir_all(parent).map_err(|cause| {
                filesystem_error("impossibile creare i manifest", parent, cause)
            })?;
            let mut bytes = serde_json::to_vec_pretty(&payload).map_err(|cause| {
                OperationError::new(
                    ErrorCode::Configuration,
                    format!("manifest contenitori non valido: {cause}"),
                )
            })?;
            bytes.push(b'\n');
            fs::write(&result.manifest, bytes).map_err(|cause| {
                filesystem_error(
                    "impossibile scrivere il manifest contenitori",
                    &result.manifest,
                    cause,
                )
            })
        }
    )
}

fn known_suffix(path: &Path) -> String {
    trace_container_call!(
        "containers::known_suffix",
        [TraceField::new("path", TraceValue::path(path))],
        {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let name = name.to_lowercase();
            if name.ends_with(".tar.xz") {
                return ".tar.xz".to_owned();
            }
            Path::new(&name)
                .extension()
                .map_or_else(String::new, |extension| {
                    format!(".{}", extension.to_string_lossy())
                })
        }
    )
}

fn find_program(name: &str) -> Option<PathBuf> {
    trace_container_call!(
        "containers::find_program",
        [
            TraceField::new("program_name", trace_sensitive_text(name)),
            TraceField::new(
                "path_search_environment",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
            TraceField::new(
                "pathext_environment",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        {
            let extensions: Vec<_> = if cfg!(windows) && Path::new(name).extension().is_none() {
                std::env::var_os("PATHEXT")
                    .map(|value| {
                        value
                            .to_string_lossy()
                            .split(';')
                            .filter(|extension| !extension.is_empty())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_else(|| {
                        vec![".COM".to_owned(), ".EXE".to_owned(), ".BAT".to_owned()]
                    })
            } else {
                vec![String::new()]
            };
            std::env::split_paths(&std::env::var_os("PATH")?).find_map(|directory| {
                extensions.iter().find_map(|extension| {
                    let candidate = directory.join(format!("{name}{extension}"));
                    candidate.is_file().then_some(candidate)
                })
            })
        }
    )
}

fn sevenz_error(message: impl Into<String>) -> sevenz_rust2::Error {
    let message = message.into();
    trace_container_call!(
        "containers::sevenz_error",
        [TraceField::new("message", trace_sensitive_text(&message))],
        sevenz_rust2::Error::Other(message.into())
    )
}

fn filesystem_error(action: &str, path: &Path, cause: io::Error) -> OperationError {
    trace_container_call!(
        "containers::filesystem_error",
        [
            TraceField::new("action", trace_sensitive_text(action)),
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("cause", trace_sensitive_text(&cause.to_string()))
        ],
        OperationError::new(
            ErrorCode::Configuration,
            format!("{action} {}: {cause}", path.display())
        )
    )
}
