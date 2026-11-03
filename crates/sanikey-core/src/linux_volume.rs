//! Lettura e controllo dei metadati di un volume Linux prima della copia USB.

use crate::config::AccountsConfig;
use crate::operation::{ErrorCode, OperationError};
use rustix::fs::statvfs;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Metadati letti dal mount Linux che contiene il target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxVolumeInfo {
    /// Punto di mount restituito da `findmnt`, se disponibile.
    pub mount_target: Option<PathBuf>,
    /// Tipo filesystem restituito da `findmnt`, se disponibile.
    pub filesystem: Option<String>,
    /// UUID filesystem restituito da `findmnt`, se disponibile.
    pub uuid: Option<String>,
    /// Byte liberi restituiti dal filesystem.
    pub available_bytes: u64,
}

/// Legge spazio libero e, quando `findmnt` è disponibile, i metadati del mount.
///
/// La funzione richiede un target già esistente e non crea directory né scrive
/// sul volume. Un risultato senza metadati mount è ammesso; il validatore lo
/// rifiuta se la configurazione richiede UUID o exFAT.
pub fn inspect_linux_volume(target: &Path) -> Result<LinuxVolumeInfo, OperationError> {
    if !target.is_dir() {
        return Err(usb_error(
            "il target Linux deve essere una directory esistente",
        ));
    }

    let statistics = statvfs(target)
        .map_err(|error| usb_error(format!("lettura spazio del volume Linux fallita: {error}")))?;
    let block_size = if statistics.f_frsize == 0 {
        statistics.f_bsize
    } else {
        statistics.f_frsize
    };
    let available_bytes = statistics
        .f_bfree
        .checked_mul(block_size)
        .ok_or_else(|| usb_error("spazio disponibile Linux fuori intervallo"))?;

    let mount = find_mount_info(target);
    Ok(LinuxVolumeInfo {
        mount_target: mount
            .as_ref()
            .and_then(|info| info.get("target"))
            .map(PathBuf::from),
        filesystem: mount.as_ref().and_then(|info| info.get("fstype")).cloned(),
        uuid: mount.and_then(|info| info.get("uuid").cloned()),
        available_bytes,
    })
}

/// Controlla UUID, filesystem e margine libero secondo la configurazione.
///
/// `image_bytes` è la dimensione dell'immagine già preparata; la funzione
/// aggiunge il margine configurato e resta in sola lettura.
pub fn validate_linux_volume(
    config: &AccountsConfig,
    target: &Path,
    image_bytes: u64,
) -> Result<LinuxVolumeInfo, OperationError> {
    let info = inspect_linux_volume(target)?;
    validate_volume_info(config, target, image_bytes, info)
}

fn validate_volume_info(
    config: &AccountsConfig,
    target: &Path,
    image_bytes: u64,
    info: LinuxVolumeInfo,
) -> Result<LinuxVolumeInfo, OperationError> {
    let reserve_bytes = u64::try_from(config.usb.min_free_space_mb)
        .ok()
        .and_then(|megabytes| megabytes.checked_mul(1024 * 1024))
        .ok_or_else(|| usb_error("margine minimo dello spazio USB non valido"))?;
    let required_bytes = image_bytes
        .checked_add(reserve_bytes)
        .ok_or_else(|| usb_error("spazio richiesto per l'export USB fuori intervallo"))?;
    if info.available_bytes < required_bytes {
        return Err(usb_error(
            "target USB Linux con spazio libero insufficiente",
        ));
    }

    let Some(filesystem) = info.filesystem.as_deref() else {
        if config.usb.usb_uuid.is_some() || config.usb.require_exfat {
            return Err(usb_error(
                "impossibile determinare i metadati filesystem Linux richiesti",
            ));
        }
        return Ok(info);
    };

    let expected_uuid = expected_uuid(config, is_likely_physical_target(target));
    if expected_uuid.is_some_and(|expected| info.uuid.as_deref() != Some(expected)) {
        return Err(usb_error("UUID filesystem USB Linux non corrispondente"));
    }
    if config.usb.require_exfat && filesystem != "exfat" {
        return Err(usb_error(
            "il filesystem del volume USB Linux deve essere exFAT",
        ));
    }
    Ok(info)
}

fn expected_uuid(config: &AccountsConfig, enforce_person_uuid: bool) -> Option<&str> {
    if let Some(uuid) = config.usb.usb_uuid.as_deref() {
        return Some(uuid);
    }
    if !enforce_person_uuid {
        return None;
    }
    let mut uuids = config
        .enabled_people()
        .map(|person| person.usb_uuid.as_str())
        .filter(|uuid| !uuid.is_empty());
    let first = uuids.next()?;
    uuids.all(|uuid| uuid == first).then_some(first)
}

fn is_likely_physical_target(target: &Path) -> bool {
    let resolved = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    resolved.starts_with("/media") || resolved.starts_with("/run/media")
}

fn find_mount_info(target: &Path) -> Option<std::collections::BTreeMap<String, String>> {
    let output = Command::new("findmnt")
        .args(["--json", "--output", "TARGET,FSTYPE,UUID", "--target"])
        .arg(target)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_findmnt_output(&output.stdout)
}

fn parse_findmnt_output(bytes: &[u8]) -> Option<std::collections::BTreeMap<String, String>> {
    let payload: Value = serde_json::from_slice(bytes).ok()?;
    let selected = payload
        .get("filesystems")?
        .as_array()?
        .first()?
        .as_object()?;
    let mut info = std::collections::BTreeMap::new();
    for key in ["target", "fstype", "uuid"] {
        let value = selected
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !value.is_empty() {
            info.insert(key.to_owned(), value.to_owned());
        }
    }
    Some(info)
}

fn usb_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Usb, message)
}

#[cfg(test)]
mod tests {
    use super::{
        LinuxVolumeInfo, expected_uuid, inspect_linux_volume, is_likely_physical_target,
        parse_findmnt_output, validate_volume_info,
    };
    use crate::config::{AccountsConfig, PersonConfig, UsbConfig};
    use std::path::PathBuf;

    fn config(global_uuid: Option<&str>, people: &[(&str, bool)]) -> AccountsConfig {
        AccountsConfig {
            config_version: 1,
            people: people
                .iter()
                .map(|(uuid, enabled)| PersonConfig {
                    id: "fixture-person".to_owned(),
                    display_name: "Persona sintetica".to_owned(),
                    source_documents: PathBuf::new(),
                    metadata_directory: PathBuf::new(),
                    local_build: PathBuf::new(),
                    usb_uuid: (*uuid).to_owned(),
                    enabled: *enabled,
                    ui: Default::default(),
                    search: Default::default(),
                    ingestion: Default::default(),
                })
                .collect(),
            path: PathBuf::new(),
            ui: Default::default(),
            search: Default::default(),
            ingestion: Default::default(),
            usb: UsbConfig {
                usb_uuid: global_uuid.map(str::to_owned),
                require_exfat: false,
                min_free_space_mb: 1,
                copy_strategy: "python".to_owned(),
            },
        }
    }

    fn volume(
        filesystem: Option<&str>,
        uuid: Option<&str>,
        available_bytes: u64,
    ) -> LinuxVolumeInfo {
        LinuxVolumeInfo {
            mount_target: Some(PathBuf::from("/media/synthetic")),
            filesystem: filesystem.map(str::to_owned),
            uuid: uuid.map(str::to_owned),
            available_bytes,
        }
    }

    #[test]
    fn parses_findmnt_json_fields_and_missing_values() {
        let parsed = parse_findmnt_output(
            br#"{"filesystems":[{"target":"/media/synthetic","fstype":"exfat","uuid":"FIXTURE-UUID"}]}"#,
        )
        .expect("fixture JSON is valid");

        assert_eq!(
            parsed.get("target").map(String::as_str),
            Some("/media/synthetic")
        );
        assert_eq!(parsed.get("fstype").map(String::as_str), Some("exfat"));
        assert_eq!(parsed.get("uuid").map(String::as_str), Some("FIXTURE-UUID"));
        assert!(parse_findmnt_output(br#"{"filesystems":[]}"#).is_none());
        assert!(parse_findmnt_output(b"not-json").is_none());
    }

    #[test]
    fn expected_uuid_prefers_global_and_requires_shared_enabled_person_uuid_on_media() {
        let global = config(Some("GLOBAL-UUID"), &[("PERSON-UUID", true)]);
        assert_eq!(expected_uuid(&global, true), Some("GLOBAL-UUID"));

        let shared = config(None, &[("SHARED-UUID", true), ("SHARED-UUID", true)]);
        assert_eq!(expected_uuid(&shared, true), Some("SHARED-UUID"));
        assert_eq!(expected_uuid(&shared, false), None);

        let disabled_conflict = config(None, &[("SHARED-UUID", true), ("OTHER", false)]);
        assert_eq!(expected_uuid(&disabled_conflict, true), Some("SHARED-UUID"));
    }

    #[test]
    fn detects_only_linux_removable_media_prefixes() {
        assert!(is_likely_physical_target(
            PathBuf::from("/media/fixture").as_path()
        ));
        assert!(is_likely_physical_target(
            PathBuf::from("/run/media/user/fixture").as_path()
        ));
        assert!(!is_likely_physical_target(
            PathBuf::from("/tmp/fixture").as_path()
        ));
    }

    #[test]
    fn volume_info_keeps_unavailable_mount_fields_explicit() {
        let info = LinuxVolumeInfo {
            mount_target: None,
            filesystem: None,
            uuid: None,
            available_bytes: 123,
        };
        assert_eq!(info.available_bytes, 123);
        assert_eq!(info.uuid, None);
    }

    #[test]
    fn inspects_repository_filesystem_without_modifying_it() {
        let info = inspect_linux_volume(PathBuf::from(".").as_path())
            .expect("repository directory is on a readable Linux filesystem");
        assert!(info.available_bytes > 0);
    }

    #[test]
    fn validates_synthetic_volume_uuid_filesystem_and_space_constraints() {
        let mut configured = config(Some("FIXTURE-UUID"), &[]);
        configured.usb.require_exfat = true;
        let enough_space = 2 * 1024 * 1024;
        let target = PathBuf::from("/media/synthetic");

        assert!(
            validate_volume_info(
                &configured,
                &target,
                1024,
                volume(Some("exfat"), Some("FIXTURE-UUID"), enough_space),
            )
            .is_ok()
        );
        assert!(
            validate_volume_info(
                &configured,
                &target,
                1024,
                volume(Some("ext4"), Some("FIXTURE-UUID"), enough_space),
            )
            .is_err()
        );
        assert!(
            validate_volume_info(
                &configured,
                &target,
                1024,
                volume(Some("exfat"), Some("OTHER-UUID"), enough_space),
            )
            .is_err()
        );
        assert!(
            validate_volume_info(
                &configured,
                &target,
                enough_space,
                volume(Some("exfat"), Some("FIXTURE-UUID"), enough_space),
            )
            .is_err()
        );
    }

    #[test]
    fn missing_mount_metadata_is_allowed_only_without_global_mount_requirements() {
        let optional = config(None, &[("PERSON-UUID", true)]);
        let info = volume(None, None, 2 * 1024 * 1024);
        let target = PathBuf::from("/tmp/synthetic");
        assert!(validate_volume_info(&optional, &target, 0, info.clone()).is_ok());

        let required = config(Some("GLOBAL-UUID"), &[]);
        assert!(validate_volume_info(&required, &target, 0, info).is_err());
    }
}
