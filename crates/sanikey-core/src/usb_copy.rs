//! Copia portabile e aggiornamento di directory immagine USB.

#[cfg(test)]
mod tests;

use crate::logging::{
    Component as LogComponent, TraceField, TraceOmission, TraceValue, trace_current,
    trace_sensitive_text,
};
use crate::operation::{
    CancellationToken, ErrorCode, OperationContext, OperationError, OperationPhase, OperationRun,
    run_operation,
};
use crate::simulated_export::validate_txt_simulation;
use crate::usb_image::validate_usb_image;
#[cfg(target_os = "linux")]
use crate::{config::AccountsConfig, linux_volume::validate_linux_volume};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);

macro_rules! trace_usb_call {
    ($function:literal, [$($field:expr),* $(,)?], $body:expr, $output:expr) => {{
        let trace = trace_current(LogComponent::Core, $function, || vec![$($field),*]);
        let result = run_usb_body(|| $body);
        if let Some(trace) = trace {
            let output = $output(&result);
            let _ = trace.finish(TraceValue::Data(json!({
                "output": output.to_json(),
            })));
        }
        result
    }};
}

fn run_usb_body<T>(body: impl FnOnce() -> T) -> T {
    body()
}

fn trace_paths(paths: &[PathBuf]) -> TraceValue {
    TraceValue::Data(Value::Array(
        paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                json!({
                    "index": index,
                    "path": TraceValue::path(path).to_json(),
                })
            })
            .collect(),
    ))
}

fn trace_source_tree(tree: &SourceTree) -> TraceValue {
    TraceValue::Data(json!({
        "directories": trace_paths(&tree.directories).to_json(),
        "files": trace_paths(&tree.files).to_json(),
    }))
}

fn trace_target_tree(tree: &TargetTree) -> TraceValue {
    TraceValue::Data(json!({
        "directories": trace_paths(&tree.directories).to_json(),
        "files": trace_paths(&tree.files).to_json(),
    }))
}

fn trace_tree_entry_state(
    directories: &[PathBuf],
    files: &[PathBuf],
    root_call: bool,
    directory_count: usize,
    file_count: usize,
) -> TraceValue {
    if root_call {
        TraceValue::Data(json!({
            "kind": "root_snapshot",
            "directories": trace_paths(directories).to_json(),
            "files": trace_paths(files).to_json(),
        }))
    } else {
        TraceValue::Data(json!({
            "kind": "root_snapshot_plus_indexed_deltas",
            "directories_length_at_entry": directory_count,
            "files_length_at_entry": file_count,
        }))
    }
}

fn trace_copy_result(result: &Result<UsbCopyResult, OperationError>) -> TraceValue {
    match result {
        Ok(result) => TraceValue::Data(json!({
            "root": TraceValue::path(&result.root).to_json(),
            "copied_files": result.copied_files,
            "reused_files": result.reused_files,
            "removed_files": result.removed_files,
            "bytes_copied": result.bytes_copied,
        })),
        Err(_) => TraceValue::Data(json!(null)),
    }
}

fn trace_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> TraceValue {
    trace_sensitive_text(&format!("{value:#?}"))
}

fn trace_result_debug<T: std::fmt::Debug>(result: &Result<T, OperationError>) -> TraceValue {
    match result {
        Ok(value) => trace_debug(value),
        Err(error) => trace_error(error),
    }
}

fn trace_error(error: &OperationError) -> TraceValue {
    TraceValue::Data(json!({
        "state": "error",
        "code": error.code().as_str(),
        "message": trace_sensitive_text(error.message()).to_json(),
    }))
}

fn trace_run_result(result: &OperationRun<UsbCopyResult>) -> TraceValue {
    TraceValue::Data(json!({
        "outcome": match &result.outcome {
            crate::operation::OperationOutcome::Succeeded(copy) => json!({
                "state": "succeeded",
                "root": TraceValue::path(&copy.root).to_json(),
                "copied_files": copy.copied_files,
                "reused_files": copy.reused_files,
                "removed_files": copy.removed_files,
                "bytes_copied": copy.bytes_copied,
            }),
            crate::operation::OperationOutcome::Failed(error) => json!({
                "state": "failed",
                "error": trace_error(error).to_json(),
            }),
            crate::operation::OperationOutcome::Cancelled => json!({"state":"cancelled"}),
        },
        "events": result.events.iter().map(|event| match event {
            crate::operation::OperationEvent::Started { operation } => json!({"state":"started","operation":operation}),
            crate::operation::OperationEvent::Progress { phase, completed, total } => json!({
                "state":"progress","phase":format!("{phase:?}"),"completed":completed,"total":total,
            }),
            crate::operation::OperationEvent::Succeeded => json!({"state":"succeeded"}),
            crate::operation::OperationEvent::Failed(error) => json!({"state":"failed","error":trace_error(error).to_json()}),
            crate::operation::OperationEvent::Cancelled => json!({"state":"cancelled"}),
        }).collect::<Vec<_>>(),
        "successful_paths": match &result.outcome {
            crate::operation::OperationOutcome::Succeeded(copy) => json!({
                "root": TraceValue::path(&copy.root).to_json(),
                "copied_files": copy.copied_files,
                "reused_files": copy.reused_files,
                "removed_files": copy.removed_files,
                "bytes_copied": copy.bytes_copied,
            }),
            _ => Value::Null,
        },
    }))
}

#[cfg(target_os = "linux")]
fn trace_config_paths(config: &AccountsConfig) -> TraceValue {
    TraceValue::Data(json!({
        "config_path": TraceValue::path(&config.path).to_json(),
        "people": config.people.iter().map(|person| json!({
            "id": person.id,
            "source_documents": TraceValue::path(&person.source_documents).to_json(),
            "metadata_directory": TraceValue::path(&person.metadata_directory).to_json(),
            "local_build": TraceValue::path(&person.local_build).to_json(),
            "background_image": person.ui.background_image.as_deref().map(|path| TraceValue::path(path).to_json()),
            "search_dictionary": person.search.dictionary.as_deref().map(|path| TraceValue::path(path).to_json()),
        })).collect::<Vec<_>>(),
    }))
}

/// Esito della sincronizzazione di una directory immagine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsbCopyResult {
    /// Directory aggiornata.
    pub root: PathBuf,
    /// File copiati o sostituiti.
    pub copied_files: usize,
    /// File identici riutilizzati senza riscriverli.
    pub reused_files: usize,
    /// File obsoleti rimossi dopo le copie riuscite.
    pub removed_files: usize,
    /// Byte scritti nei nuovi file.
    pub bytes_copied: u64,
}

#[derive(Default)]
struct SourceTree {
    directories: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

#[derive(Default)]
struct TargetTree {
    directories: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

/// Aggiorna una directory da un'immagine già preparata, senza dipendere da rsync.
///
/// Prima di scrivere verifica che sorgente e destinazione siano directory
/// distinte, che non si sovrappongano e che gli alberi non contengano link
/// simbolici o collisioni tra file e directory. I file con lo stesso SHA-256
/// restano intatti; i file obsoleti vengono rimossi solo dopo il completamento
/// delle copie. In caso di errore una nuova esecuzione può completare
/// l'aggiornamento senza modificare la sorgente.
///
/// L'operazione emette eventi di progresso per ciascun file sorgente e osserva
/// l'annullamento fra i file. Un annullamento può lasciare una destinazione
/// parzialmente aggiornata; rieseguire la stessa operazione la riallinea.
///
/// # Errors
///
/// Restituisce un esito `Failed` con `ErrorCode::Usb` se la validazione, la
/// lettura, la copia o la rimozione degli artefatti fallisce.
pub fn mirror_directory(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    cancellation: &CancellationToken,
) -> OperationRun<UsbCopyResult> {
    trace_usb_call!(
        "usb_copy::mirror_directory",
        [
            TraceField::new("source", TraceValue::path(source.as_ref())),
            TraceField::new("target", TraceValue::path(target.as_ref())),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(cancellation.is_cancelled())),
            ),
        ],
        run_operation("usb.copy", cancellation, |context| {
            mirror_directory_inner(source.as_ref(), target.as_ref(), context)
        }),
        trace_run_result
    )
}

/// Copia un'immagine TXT già costruita, convalida manifest e link prima e dopo.
///
/// La verifica iniziale impedisce di pubblicare un'immagine sorgente corrotta;
/// quella finale garantisce che il target contenga l'immagine verificata.
/// L'annullamento resta distinto dal successo e può lasciare un target parziale
/// da riallineare con una successiva esecuzione.
///
/// # Errors
///
/// Restituisce un esito `Failed` USB se la sorgente non è valida, la copia
/// fallisce o il target non supera la convalida finale.
pub fn mirror_validated_txt_image(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    cancellation: &CancellationToken,
) -> OperationRun<UsbCopyResult> {
    trace_usb_call!(
        "usb_copy::mirror_validated_txt_image",
        [
            TraceField::new("source", TraceValue::path(source.as_ref())),
            TraceField::new("target", TraceValue::path(target.as_ref())),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(cancellation.is_cancelled())),
            ),
        ],
        run_operation("usb.copy.validate", cancellation, |context| {
            mirror_validated_txt_image_inner(source.as_ref(), target.as_ref(), context)
        }),
        trace_run_result
    )
}

/// Copia un'immagine USB completa, verificando manifest, checksum e link prima e dopo.
///
/// # Errors
///
/// Restituisce un esito `Failed` USB se la sorgente non è valida, la copia fallisce
/// o il target non supera la convalida finale.
pub fn mirror_validated_usb_image(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    cancellation: &CancellationToken,
) -> OperationRun<UsbCopyResult> {
    trace_usb_call!(
        "usb_copy::mirror_validated_usb_image",
        [
            TraceField::new("source", TraceValue::path(source.as_ref())),
            TraceField::new("target", TraceValue::path(target.as_ref())),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(cancellation.is_cancelled())),
            ),
        ],
        run_operation("usb.copy.validate_image", cancellation, |context| {
            if context.is_cancelled() {
                return Ok(empty_result(target.as_ref().to_path_buf()));
            }
            mirror_validated_usb_image_inner(source.as_ref(), target.as_ref(), context)
        }),
        trace_run_result
    )
}

/// Copia un'immagine TXT su un volume Linux già esistente e lo ricontrolla.
///
/// Valida il manifest sorgente, calcola lo spazio richiesto, verifica volume,
/// identità, filesystem e spazio prima della copia, poi valida checksum e link
/// sul target e rilegge il volume. Un cambio di mount o identità durante la
/// copia fallisce l'operazione. Non crea il target.
///
/// # Errors
///
/// Restituisce un esito `Failed` USB quando una verifica o la copia fallisce.
#[cfg(target_os = "linux")]
pub fn mirror_validated_txt_image_on_linux_volume(
    config: &AccountsConfig,
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    cancellation: &CancellationToken,
) -> OperationRun<UsbCopyResult> {
    trace_usb_call!(
        "usb_copy::mirror_validated_txt_image_on_linux_volume",
        [
            TraceField::new("config", trace_sensitive_text(&format!("{config:#?}"))),
            TraceField::new("config_paths", trace_config_paths(config)),
            TraceField::new("source", TraceValue::path(source.as_ref())),
            TraceField::new("target", TraceValue::path(target.as_ref())),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(cancellation.is_cancelled())),
            ),
        ],
        run_operation("usb.copy.validate_linux_volume", cancellation, |context| {
            if context.is_cancelled() {
                return Ok(empty_result(target.as_ref().to_path_buf()));
            }
            let source_input = source.as_ref();
            reject_symlink_image_root(source_input)?;
            let source = fs::canonicalize(source_input).map_err(|cause| {
                filesystem_error("immagine sorgente non leggibile", source_input, cause)
            })?;
            validate_txt_simulation(&source).map_err(|cause| {
                usb_error(format!(
                    "immagine USB sorgente non valida: {}",
                    cause.message()
                ))
            })?;
            let image_bytes = source_directory_bytes(&source)?;
            let target = target.as_ref();
            let before = validate_linux_volume(config, target, image_bytes).map_err(|error| {
                usb_error(format!(
                    "preflight volume Linux fallito: {}",
                    error.message()
                ))
            })?;
            let result = mirror_directory_inner(&source, target, context)?;
            if context.is_cancelled() {
                return Ok(result);
            }
            context.set_phase(OperationPhase::Finalizing);
            validate_txt_simulation(&result.root)
                .map_err(|cause| usb_error(format!("copia USB non valida: {}", cause.message())))?;
            let after = validate_linux_volume(config, target, 0).map_err(|error| {
                usb_error(format!(
                    "verifica finale volume Linux fallita: {}",
                    error.message()
                ))
            })?;
            if before.mount_target != after.mount_target
                || before.filesystem != after.filesystem
                || before.uuid != after.uuid
            {
                return Err(usb_error(
                    "mount o identità del volume Linux cambiati durante la copia USB",
                ));
            }
            context.report_progress(1, Some(1));
            Ok(result)
        }),
        trace_run_result
    )
}

/// Copia un'immagine USB completa su un volume Linux già esistente e ne ricontrolla l'identità.
///
/// # Errors
///
/// Restituisce un esito `Failed` USB quando una verifica o la copia fallisce.
#[cfg(target_os = "linux")]
pub fn mirror_validated_usb_image_on_linux_volume(
    config: &AccountsConfig,
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    cancellation: &CancellationToken,
) -> OperationRun<UsbCopyResult> {
    trace_usb_call!(
        "usb_copy::mirror_validated_usb_image_on_linux_volume",
        [
            TraceField::new("config", trace_sensitive_text(&format!("{config:#?}"))),
            TraceField::new("config_paths", trace_config_paths(config)),
            TraceField::new("source", TraceValue::path(source.as_ref())),
            TraceField::new("target", TraceValue::path(target.as_ref())),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(cancellation.is_cancelled()))
            ),
        ],
        run_operation(
            "usb.copy.validate_image_linux_volume",
            cancellation,
            |context| {
                if context.is_cancelled() {
                    return Ok(empty_result(target.as_ref().to_path_buf()));
                }
                let source_input = source.as_ref();
                reject_symlink_image_root(source_input)?;
                let source = fs::canonicalize(source_input).map_err(|cause| {
                    filesystem_error("immagine sorgente non leggibile", source_input, cause)
                })?;
                validate_usb_image(&source).map_err(|cause| {
                    usb_error(format!(
                        "immagine USB sorgente non valida: {}",
                        cause.message()
                    ))
                })?;
                let image_bytes = source_directory_bytes(&source)?;
                let volume_root = target.as_ref();
                let before =
                    validate_linux_volume(config, volume_root, image_bytes).map_err(|error| {
                        usb_error(format!(
                            "preflight volume Linux fallito: {}",
                            error.message()
                        ))
                    })?;
                let image_target = volume_root.join("SaniKey");
                let result = mirror_directory_inner(&source, &image_target, context)?;
                if context.is_cancelled() {
                    return Ok(result);
                }
                context.set_phase(OperationPhase::Finalizing);
                validate_usb_image(&result.root).map_err(|cause| {
                    usb_error(format!("copia USB non valida: {}", cause.message()))
                })?;
                let after = validate_linux_volume(config, volume_root, 0).map_err(|error| {
                    usb_error(format!(
                        "verifica finale volume Linux fallita: {}",
                        error.message()
                    ))
                })?;
                if before.mount_target != after.mount_target
                    || before.filesystem != after.filesystem
                    || before.uuid != after.uuid
                {
                    return Err(usb_error(
                        "mount o identità del volume Linux cambiati durante la copia USB",
                    ));
                }
                context.report_progress(1, Some(1));
                Ok(result)
            },
        ),
        trace_run_result
    )
}

fn mirror_validated_txt_image_inner(
    source: &Path,
    target: &Path,
    context: &mut OperationContext<'_>,
) -> Result<UsbCopyResult, OperationError> {
    trace_usb_call!(
        "usb_copy::mirror_validated_txt_image_inner",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(context.is_cancelled())),
            ),
        ],
        {
            let source = fs::canonicalize(source).map_err(|cause| {
                filesystem_error("immagine sorgente non leggibile", source, cause)
            })?;
            validate_txt_simulation(&source).map_err(|cause| {
                usb_error(format!(
                    "immagine USB sorgente non valida: {}",
                    cause.message()
                ))
            })?;
            let result = mirror_directory_inner(&source, target, context)?;
            if context.is_cancelled() {
                return Ok(result);
            }
            context.set_phase(OperationPhase::Finalizing);
            validate_txt_simulation(&result.root)
                .map_err(|cause| usb_error(format!("copia USB non valida: {}", cause.message())))?;
            context.report_progress(1, Some(1));
            Ok(result)
        },
        trace_copy_result
    )
}

fn mirror_validated_usb_image_inner(
    source: &Path,
    target: &Path,
    context: &mut OperationContext<'_>,
) -> Result<UsbCopyResult, OperationError> {
    trace_usb_call!(
        "usb_copy::mirror_validated_usb_image_inner",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(context.is_cancelled())),
            ),
        ],
        {
            reject_symlink_image_root(source)?;
            let source = fs::canonicalize(source).map_err(|cause| {
                filesystem_error("immagine sorgente non leggibile", source, cause)
            })?;
            validate_usb_image(&source).map_err(|cause| {
                usb_error(format!(
                    "immagine USB sorgente non valida: {}",
                    cause.message()
                ))
            })?;
            let result = mirror_directory_inner(&source, target, context)?;
            if context.is_cancelled() {
                return Ok(result);
            }
            context.set_phase(OperationPhase::Finalizing);
            validate_usb_image(&result.root)
                .map_err(|cause| usb_error(format!("copia USB non valida: {}", cause.message())))?;
            context.report_progress(1, Some(1));
            Ok(result)
        },
        trace_copy_result
    )
}

fn reject_symlink_image_root(source: &Path) -> Result<(), OperationError> {
    trace_usb_call!(
        "usb_copy::reject_symlink_image_root",
        [TraceField::new("source", TraceValue::path(source))],
        {
            let metadata = fs::symlink_metadata(source).map_err(|cause| {
                filesystem_error("immagine sorgente non leggibile", source, cause)
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(usb_error(
                    "la radice dell'immagine sorgente deve essere una directory reale",
                ));
            }
            Ok(())
        },
        trace_result_debug
    )
}

#[cfg(target_os = "linux")]
fn source_directory_bytes(source: &Path) -> Result<u64, OperationError> {
    trace_usb_call!(
        "usb_copy::source_directory_bytes",
        [TraceField::new("source", TraceValue::path(source))],
        {
            let mut tree = SourceTree::default();
            collect_source_tree(source, source, &mut tree)?;
            tree.files.iter().try_fold(0_u64, |total, relative| {
                let path = source.join(relative);
                let length = fs::metadata(&path)
                    .map_err(|cause| filesystem_error("file immagine non leggibile", &path, cause))?
                    .len();
                total
                    .checked_add(length)
                    .ok_or_else(|| usb_error("dimensione immagine USB fuori intervallo"))
            })
        },
        trace_result_debug
    )
}

fn mirror_directory_inner(
    source: &Path,
    target: &Path,
    context: &mut OperationContext<'_>,
) -> Result<UsbCopyResult, OperationError> {
    trace_usb_call!(
        "usb_copy::mirror_directory_inner",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "cancellation_requested",
                TraceValue::Data(json!(context.is_cancelled())),
            ),
        ],
        {
            context.set_phase(OperationPhase::Preparing);
            let source = fs::canonicalize(source).map_err(|cause| {
                filesystem_error("immagine sorgente non leggibile", source, cause)
            })?;
            if !source.is_dir() {
                return Err(usb_error(format!(
                    "l'immagine sorgente non è una directory: {}",
                    source.display()
                )));
            }
            let target_input = absolute_path(target)?;
            if let Ok(metadata) = fs::symlink_metadata(&target_input)
                && metadata.file_type().is_symlink()
            {
                return Err(usb_error(format!(
                    "la destinazione è un link simbolico: {}",
                    target_input.display()
                )));
            }
            let target = resolve_path(&target_input)?;
            if source == target || source.starts_with(&target) || target.starts_with(&source) {
                return Err(usb_error(format!(
                    "sorgente e destinazione si sovrappongono: {} e {}",
                    source.display(),
                    target.display()
                )));
            }
            if let Ok(metadata) = fs::symlink_metadata(&target)
                && (metadata.file_type().is_symlink() || !metadata.is_dir())
            {
                return Err(usb_error(format!(
                    "la destinazione esiste e non è una directory sicura: {}",
                    target.display()
                )));
            }

            let mut source_tree = SourceTree::default();
            collect_source_tree(&source, &source, &mut source_tree)?;
            let source_files: HashSet<_> = source_tree.files.iter().cloned().collect();
            let source_directories: HashSet<_> = source_tree.directories.iter().cloned().collect();
            let target_tree = if target.is_dir() {
                let mut tree = TargetTree::default();
                collect_target_tree(&target, &target, &mut tree)?;
                validate_type_collisions(&source_tree, &tree)?;
                tree
            } else {
                TargetTree::default()
            };

            if context.is_cancelled() {
                return Ok(empty_result(target));
            }
            fs::create_dir_all(&target).map_err(|cause| {
                filesystem_error("impossibile creare la destinazione", &target, cause)
            })?;
            for relative in &source_tree.directories {
                let directory = target.join(relative);
                fs::create_dir_all(&directory).map_err(|cause| {
                    filesystem_error(
                        "impossibile creare una directory immagine",
                        &directory,
                        cause,
                    )
                })?;
            }

            context.set_phase(OperationPhase::Processing);
            let total = source_tree.files.len() as u64;
            let mut result = empty_result(target.clone());
            for (index, relative) in source_tree.files.iter().enumerate() {
                if context.is_cancelled() {
                    return Ok(result);
                }
                let source_file = source.join(relative);
                let target_file = target.join(relative);
                let source_digest = sha256_file(&source_file)?;
                if target_file.is_file() && sha256_file(&target_file)? == source_digest {
                    result.reused_files += 1;
                } else {
                    let bytes = copy_file_atomically(&source_file, &target_file, &source_digest)?;
                    result.copied_files += 1;
                    result.bytes_copied = result.bytes_copied.saturating_add(bytes);
                }
                context.report_progress((index + 1) as u64, Some(total));
            }

            if context.is_cancelled() {
                return Ok(result);
            }
            context.set_phase(OperationPhase::Finalizing);
            for relative in &target_tree.files {
                if !source_files.contains(relative) {
                    let path = target.join(relative);
                    fs::remove_file(&path).map_err(|cause| {
                        filesystem_error("impossibile rimuovere un file obsoleto", &path, cause)
                    })?;
                    result.removed_files += 1;
                }
            }
            let mut stale_directories: Vec<_> = target_tree
                .directories
                .iter()
                .filter(|relative| !source_directories.contains(*relative))
                .collect();
            stale_directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
            for relative in stale_directories {
                let path = target.join(relative);
                fs::remove_dir(&path).map_err(|cause| {
                    filesystem_error("impossibile rimuovere una directory obsoleta", &path, cause)
                })?;
            }
            Ok(result)
        },
        trace_copy_result
    )
}

fn collect_source_tree(
    root: &Path,
    directory: &Path,
    tree: &mut SourceTree,
) -> Result<(), OperationError> {
    let root_call = root == directory;
    let directories_before = tree.directories.len();
    let files_before = tree.files.len();
    let mut directories_added = Vec::new();
    let mut files_added = Vec::new();
    trace_usb_call!(
        "usb_copy::collect_source_tree",
        [
            TraceField::new("root", TraceValue::path(root)),
            TraceField::new("directory", TraceValue::path(directory)),
            TraceField::new(
                "tree_before",
                trace_tree_entry_state(
                    &tree.directories,
                    &tree.files,
                    root_call,
                    directories_before,
                    files_before,
                ),
            ),
        ],
        {
            let mut entries = fs::read_dir(directory)
                .map_err(|cause| {
                    filesystem_error("immagine sorgente non leggibile", directory, cause)
                })?
                .map(|entry| {
                    entry.map_err(|cause| {
                        filesystem_error("elemento sorgente non leggibile", directory, cause)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .expect("elemento sotto la radice")
                    .to_path_buf();
                let kind = entry.file_type().map_err(|cause| {
                    filesystem_error("tipo elemento sorgente non leggibile", &path, cause)
                })?;
                if kind.is_symlink() {
                    return Err(usb_error(format!(
                        "link simbolico non ammesso nell'immagine: {}",
                        path.display()
                    )));
                }
                if kind.is_dir() {
                    tree.directories.push(relative);
                    let index = tree.directories.len() - 1;
                    directories_added.push(json!({
                        "index": index,
                        "path": TraceValue::path(&tree.directories[index]).to_json(),
                    }));
                    collect_source_tree(root, &path, tree)?;
                } else if kind.is_file() {
                    tree.files.push(relative);
                    let index = tree.files.len() - 1;
                    files_added.push(json!({
                        "index": index,
                        "path": TraceValue::path(&tree.files[index]).to_json(),
                    }));
                } else {
                    return Err(usb_error(format!(
                        "tipo file non supportato nell'immagine: {}",
                        path.display()
                    )));
                }
            }
            Ok(())
        },
        |result: &Result<(), OperationError>| TraceValue::Data(json!({
            "result": trace_result_debug(result).to_json(),
            "directories_added": directories_added,
            "files_added": files_added,
            "directories_after": if root_call { Some(trace_paths(&tree.directories).to_json()) } else { None },
            "files_after": if root_call { Some(trace_paths(&tree.files).to_json()) } else { None },
        }))
    )
}

fn collect_target_tree(
    root: &Path,
    directory: &Path,
    tree: &mut TargetTree,
) -> Result<(), OperationError> {
    let root_call = root == directory;
    let directories_before = tree.directories.len();
    let files_before = tree.files.len();
    let mut directories_added = Vec::new();
    let mut files_added = Vec::new();
    trace_usb_call!(
        "usb_copy::collect_target_tree",
        [
            TraceField::new("root", TraceValue::path(root)),
            TraceField::new("directory", TraceValue::path(directory)),
            TraceField::new(
                "tree_before",
                trace_tree_entry_state(
                    &tree.directories,
                    &tree.files,
                    root_call,
                    directories_before,
                    files_before,
                ),
            ),
        ],
        {
            let mut entries = fs::read_dir(directory)
                .map_err(|cause| filesystem_error("destinazione non leggibile", directory, cause))?
                .map(|entry| {
                    entry.map_err(|cause| {
                        filesystem_error("elemento destinazione non leggibile", directory, cause)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .expect("elemento sotto la radice")
                    .to_path_buf();
                let kind = entry.file_type().map_err(|cause| {
                    filesystem_error("tipo elemento destinazione non leggibile", &path, cause)
                })?;
                if kind.is_symlink() {
                    return Err(usb_error(format!(
                        "link simbolico non ammesso nella destinazione: {}",
                        path.display()
                    )));
                }
                if kind.is_dir() {
                    tree.directories.push(relative);
                    let index = tree.directories.len() - 1;
                    directories_added.push(json!({
                        "index": index,
                        "path": TraceValue::path(&tree.directories[index]).to_json(),
                    }));
                    collect_target_tree(root, &path, tree)?;
                } else if kind.is_file() {
                    tree.files.push(relative);
                    let index = tree.files.len() - 1;
                    files_added.push(json!({
                        "index": index,
                        "path": TraceValue::path(&tree.files[index]).to_json(),
                    }));
                } else {
                    return Err(usb_error(format!(
                        "tipo file non supportato nella destinazione: {}",
                        path.display()
                    )));
                }
            }
            Ok(())
        },
        |result: &Result<(), OperationError>| TraceValue::Data(json!({
            "result": trace_result_debug(result).to_json(),
            "directories_added": directories_added,
            "files_added": files_added,
            "directories_after": if root_call { Some(trace_paths(&tree.directories).to_json()) } else { None },
            "files_after": if root_call { Some(trace_paths(&tree.files).to_json()) } else { None },
        }))
    )
}

fn validate_type_collisions(
    source: &SourceTree,
    target: &TargetTree,
) -> Result<(), OperationError> {
    trace_usb_call!(
        "usb_copy::validate_type_collisions",
        [
            TraceField::new("source_tree", trace_source_tree(source)),
            TraceField::new("target_tree", trace_target_tree(target)),
        ],
        {
            let source_files: HashSet<_> = source.files.iter().collect();
            let source_directories: HashSet<_> = source.directories.iter().collect();
            for relative in &target.files {
                if source_directories.contains(relative) {
                    return Err(usb_error(format!(
                        "file in conflitto con directory sorgente: {}",
                        relative.display()
                    )));
                }
            }
            for relative in &target.directories {
                if source_files.contains(relative) {
                    return Err(usb_error(format!(
                        "directory in conflitto con file sorgente: {}",
                        relative.display()
                    )));
                }
            }
            Ok(())
        },
        trace_result_debug
    )
}

fn copy_file_atomically(
    source: &Path,
    target: &Path,
    expected_digest: &str,
) -> Result<u64, OperationError> {
    let mut temporary_path = None;
    let mut verified_digest = None;
    trace_usb_call!(
        "usb_copy::copy_file_atomically",
        [
            TraceField::new("source", TraceValue::path(source)),
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new("expected_sha256", TraceValue::data(expected_digest)),
            TraceField::new(
                "transferred_contents",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ],
        {
            let parent = target
                .parent()
                .expect("file destinazione con directory padre");
            fs::create_dir_all(parent).map_err(|cause| {
                filesystem_error(
                    "impossibile creare una directory destinazione",
                    parent,
                    cause,
                )
            })?;
            let filename = target
                .file_name()
                .expect("file destinazione con nome")
                .to_string_lossy();
            let (temporary, mut output) = loop {
                let temporary = parent.join(format!(
                    ".{filename}.sanikey-{}-{}",
                    std::process::id(),
                    NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
                ));
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                {
                    Ok(output) => break (temporary, output),
                    Err(cause) if cause.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(cause) => {
                        return Err(filesystem_error(
                            "impossibile creare il file temporaneo di copia",
                            &temporary,
                            cause,
                        ));
                    }
                }
            };
            temporary_path = Some(temporary.clone());
            let copied = (|| {
                let mut input = File::open(source).map_err(|cause| {
                    filesystem_error("file sorgente non leggibile", source, cause)
                })?;
                let bytes = io::copy(&mut input, &mut output)
                    .map_err(|cause| filesystem_error("copia file non riuscita", source, cause))?;
                output.flush().map_err(|cause| {
                    filesystem_error("scrittura file temporaneo non riuscita", &temporary, cause)
                })?;
                Ok::<_, OperationError>(bytes)
            })();
            let bytes = match copied {
                Ok(bytes) => bytes,
                Err(error) => {
                    drop(output);
                    let _ = fs::remove_file(&temporary);
                    return Err(error);
                }
            };
            drop(output);
            let copied_digest = match sha256_file(&temporary) {
                Ok(digest) => digest,
                Err(error) => {
                    let _ = fs::remove_file(&temporary);
                    return Err(error);
                }
            };
            verified_digest = Some(copied_digest.clone());
            if copied_digest != expected_digest {
                let _ = fs::remove_file(&temporary);
                return Err(usb_error(format!(
                    "checksum diverso dopo la copia di {}",
                    source.display()
                )));
            }
            #[cfg(windows)]
            if target.exists() {
                if let Err(cause) = fs::remove_file(target) {
                    let _ = fs::remove_file(&temporary);
                    return Err(filesystem_error(
                        "impossibile sostituire il file destinazione",
                        target,
                        cause,
                    ));
                }
            }
            match fs::rename(&temporary, target) {
                Ok(()) => Ok(bytes),
                Err(cause) => {
                    let _ = fs::remove_file(&temporary);
                    Err(filesystem_error(
                        "impossibile finalizzare il file copiato",
                        target,
                        cause,
                    ))
                }
            }
        },
        |result: &Result<u64, OperationError>| TraceValue::Data(json!({
            "bytes_copied": result.as_ref().ok(),
            "temporary_path": temporary_path.as_deref().map(|path| TraceValue::path(path).to_json()),
            "verified_sha256": verified_digest,
            "result": trace_result_debug(result).to_json(),
        }))
    )
}

fn sha256_file(path: &Path) -> Result<String, OperationError> {
    let mut bytes_read = 0_u64;
    let trace = trace_current(LogComponent::Core, "usb_copy::sha256_file", || {
        vec![TraceField::new("path", TraceValue::path(path))]
    });
    let mut file_contents_read = trace.as_ref().map(|_| Vec::new());
    let result = (|| {
        let mut file = File::open(path)
            .map_err(|cause| filesystem_error("file non leggibile per il checksum", path, cause))?;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|cause| filesystem_error("lettura checksum non riuscita", path, cause))?;
            if count == 0 {
                break;
            }
            bytes_read = bytes_read.saturating_add(count as u64);
            if let Some(contents) = &mut file_contents_read {
                contents.extend_from_slice(&buffer[..count]);
            }
            digest.update(&buffer[..count]);
        }
        Ok(format!("{:x}", digest.finalize()))
    })();
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({
            "return": trace_result_debug(&result).to_json(),
            "output": {
                "sha256": result.as_ref().ok(),
                "bytes_read": bytes_read,
                "file_contents_read": file_contents_read,
            },
        })));
    }
    result
}

fn resolve_path(path: &Path) -> Result<PathBuf, OperationError> {
    trace_usb_call!(
        "usb_copy::resolve_path",
        [TraceField::new("path", TraceValue::path(path))],
        {
            let absolute = absolute_path(path)?;
            let mut ancestor = absolute.as_path();
            let mut missing = Vec::new();
            while !ancestor.exists() {
                let Some(name) = ancestor.file_name() else {
                    break;
                };
                missing.push(name.to_os_string());
                let Some(parent) = ancestor.parent() else {
                    break;
                };
                ancestor = parent;
            }
            let mut resolved = fs::canonicalize(ancestor).map_err(|cause| {
                filesystem_error("destinazione non risolvibile", ancestor, cause)
            })?;
            for component in missing.iter().rev() {
                resolved.push(component);
            }
            Ok(normalize(&resolved))
        },
        |result: &Result<PathBuf, OperationError>| TraceValue::Data(json!({
            "path": result.as_ref().ok().map(|path| TraceValue::path(path).to_json()),
            "result": trace_result_debug(result).to_json(),
        }))
    )
}

fn absolute_path(path: &Path) -> Result<PathBuf, OperationError> {
    trace_usb_call!(
        "usb_copy::absolute_path",
        [TraceField::new("path", TraceValue::path(path))],
        {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|cause| {
                        filesystem_error("directory corrente non leggibile", path, cause)
                    })?
                    .join(path)
            };
            Ok(normalize(&absolute))
        },
        |result: &Result<PathBuf, OperationError>| TraceValue::Data(json!({
            "path": result.as_ref().ok().map(|path| TraceValue::path(path).to_json()),
            "result": trace_result_debug(result).to_json(),
        }))
    )
}

fn normalize(path: &Path) -> PathBuf {
    trace_usb_call!(
        "usb_copy::normalize",
        [TraceField::new("path", TraceValue::path(path))],
        {
            let mut normalized = PathBuf::new();
            for component in path.components() {
                match component {
                    Component::CurDir => {}
                    Component::ParentDir => {
                        normalized.pop();
                    }
                    other => normalized.push(other.as_os_str()),
                }
            }
            normalized
        },
        |result: &PathBuf| TraceValue::path(result)
    )
}

fn empty_result(root: PathBuf) -> UsbCopyResult {
    trace_usb_call!(
        "usb_copy::empty_result",
        [TraceField::new("root", TraceValue::path(&root))],
        UsbCopyResult {
            root,
            copied_files: 0,
            reused_files: 0,
            removed_files: 0,
            bytes_copied: 0,
        },
        |result: &UsbCopyResult| TraceValue::Data(json!({
            "root": TraceValue::path(&result.root).to_json(),
            "copied_files": result.copied_files,
            "reused_files": result.reused_files,
            "removed_files": result.removed_files,
            "bytes_copied": result.bytes_copied,
        }))
    )
}

fn usb_error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    trace_usb_call!(
        "usb_copy::usb_error",
        [TraceField::new("message", trace_sensitive_text(&message))],
        OperationError::new(ErrorCode::Usb, message),
        trace_error
    )
}

fn filesystem_error(context: &str, path: &Path, cause: io::Error) -> OperationError {
    let error_data = json!({
        "kind": format!("{:?}", cause.kind()),
        "raw_os_error": cause.raw_os_error(),
        "message": trace_sensitive_text(&cause.to_string()).to_json(),
    });
    trace_usb_call!(
        "usb_copy::filesystem_error",
        [
            TraceField::new("context", TraceValue::data(context)),
            TraceField::new("path", TraceValue::path(path)),
            TraceField::new("io_error", TraceValue::Data(error_data)),
        ],
        usb_error(format!("{context} {}: {cause}", path.display())),
        trace_error
    )
}
