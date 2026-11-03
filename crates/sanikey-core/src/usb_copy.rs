//! Copia portabile e aggiornamento di directory immagine USB.

#[cfg(test)]
mod tests;

use crate::operation::{
    CancellationToken, ErrorCode, OperationContext, OperationError, OperationPhase, OperationRun,
    run_operation,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);

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
    run_operation("usb.copy", cancellation, |context| {
        mirror_directory_inner(source.as_ref(), target.as_ref(), context)
    })
}

fn mirror_directory_inner(
    source: &Path,
    target: &Path,
    context: &mut OperationContext<'_>,
) -> Result<UsbCopyResult, OperationError> {
    context.set_phase(OperationPhase::Preparing);
    let source = fs::canonicalize(source)
        .map_err(|cause| filesystem_error("immagine sorgente non leggibile", source, cause))?;
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
    fs::create_dir_all(&target)
        .map_err(|cause| filesystem_error("impossibile creare la destinazione", &target, cause))?;
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
}

fn collect_source_tree(
    root: &Path,
    directory: &Path,
    tree: &mut SourceTree,
) -> Result<(), OperationError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|cause| filesystem_error("immagine sorgente non leggibile", directory, cause))?
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
            collect_source_tree(root, &path, tree)?;
        } else if kind.is_file() {
            tree.files.push(relative);
        } else {
            return Err(usb_error(format!(
                "tipo file non supportato nell'immagine: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn collect_target_tree(
    root: &Path,
    directory: &Path,
    tree: &mut TargetTree,
) -> Result<(), OperationError> {
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
            collect_target_tree(root, &path, tree)?;
        } else if kind.is_file() {
            tree.files.push(relative);
        } else {
            return Err(usb_error(format!(
                "tipo file non supportato nella destinazione: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn validate_type_collisions(
    source: &SourceTree,
    target: &TargetTree,
) -> Result<(), OperationError> {
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
}

fn copy_file_atomically(
    source: &Path,
    target: &Path,
    expected_digest: &str,
) -> Result<u64, OperationError> {
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
    let copied = (|| {
        let mut input = File::open(source)
            .map_err(|cause| filesystem_error("file sorgente non leggibile", source, cause))?;
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
}

fn sha256_file(path: &Path) -> Result<String, OperationError> {
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
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn resolve_path(path: &Path) -> Result<PathBuf, OperationError> {
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
    let mut resolved = fs::canonicalize(ancestor)
        .map_err(|cause| filesystem_error("destinazione non risolvibile", ancestor, cause))?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(normalize(&resolved))
}

fn absolute_path(path: &Path) -> Result<PathBuf, OperationError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|cause| filesystem_error("directory corrente non leggibile", path, cause))?
            .join(path)
    };
    Ok(normalize(&absolute))
}

fn normalize(path: &Path) -> PathBuf {
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
}

fn empty_result(root: PathBuf) -> UsbCopyResult {
    UsbCopyResult {
        root,
        copied_files: 0,
        reused_files: 0,
        removed_files: 0,
        bytes_copied: 0,
    }
}

fn usb_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Usb, message)
}

fn filesystem_error(context: &str, path: &Path, cause: io::Error) -> OperationError {
    usb_error(format!("{context} {}: {cause}", path.display()))
}
