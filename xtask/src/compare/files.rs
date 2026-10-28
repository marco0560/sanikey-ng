//! Comparatori file e directory per gli artefatti di migrazione.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) fn compare_originals(reference: &Path, candidate: &Path) -> Result<(), String> {
    ensure_not_symlink(reference)?;
    ensure_not_symlink(candidate)?;
    let reference_metadata = fs::metadata(reference)
        .map_err(|error| format!("impossibile leggere {}: {error}", reference.display()))?;
    let candidate_metadata = fs::metadata(candidate)
        .map_err(|error| format!("impossibile leggere {}: {error}", candidate.display()))?;
    match (reference_metadata.is_dir(), candidate_metadata.is_dir()) {
        (false, false) => compare_file_hash(reference, candidate),
        (true, true) => {
            let reference_files = collect_files(reference)?;
            let candidate_files = collect_files(candidate)?;
            compare_file_sets(
                "originali",
                &reference_files,
                &candidate_files,
                compare_file_hash,
            )
        }
        _ => Err("un lato contiene una directory e l'altro un file".to_owned()),
    }
}

pub(super) fn ensure_not_symlink(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("impossibile leggere {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "collegamento simbolico non ammesso: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn compare_file_hash(reference: &Path, candidate: &Path) -> Result<(), String> {
    let reference_hash = sha256_file(reference)?;
    let candidate_hash = sha256_file(candidate)?;
    if reference_hash == candidate_hash {
        Ok(())
    } else {
        Err("SHA-256 differente".to_owned())
    }
}

pub(super) fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("impossibile aprire {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("impossibile leggere {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) fn collect_files(root: &Path) -> Result<BTreeMap<PathBuf, PathBuf>, String> {
    fn visit(
        root: &Path,
        current: &Path,
        files: &mut BTreeMap<PathBuf, PathBuf>,
    ) -> Result<(), String> {
        let entries = fs::read_dir(current)
            .map_err(|error| format!("impossibile leggere {}: {error}", current.display()))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("errore enumerando {}: {error}", current.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("impossibile leggere {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                return Err(format!(
                    "collegamento simbolico non ammesso: {}",
                    path.display()
                ));
            }
            if file_type.is_dir() {
                visit(root, &path, files)?;
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| format!("percorso fuori dalla radice: {error}"))?
                    .to_path_buf();
                files.insert(relative, path);
            }
        }
        Ok(())
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

pub(super) fn compare_file_sets(
    label: &str,
    reference: &BTreeMap<PathBuf, PathBuf>,
    candidate: &BTreeMap<PathBuf, PathBuf>,
    compare_file: fn(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    if reference.keys().ne(candidate.keys()) {
        return Err(format!("insieme dei percorsi {label} differente"));
    }
    for (relative, reference_path) in reference {
        let candidate_path = candidate
            .get(relative)
            .expect("gli insiemi dei percorsi sono stati confrontati");
        compare_file(reference_path, candidate_path)
            .map_err(|error| format!("{}: {error}", relative.display()))?;
    }
    Ok(())
}
