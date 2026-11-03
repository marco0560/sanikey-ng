//! Controllo Windows del volume configurato, senza euristiche sui mount Linux.

#[cfg(any(windows, test))]
use crate::config::AccountsConfig;
#[cfg(any(windows, test))]
use crate::operation::{ErrorCode, OperationError};
#[cfg(windows)]
use sanikey_windows_volume::WindowsVolumeInfo;
#[cfg(windows)]
use std::path::Path;

/// Legge e convalida il volume Windows che contiene una directory già esistente.
///
/// `usb_uuid` deve contenere il Volume GUID path Windows nella forma nativa
/// `\\?\Volume{...}\`; il valore viene confrontato letteralmente e non è
/// convertito in UUID Linux o seriale filesystem.
#[cfg(windows)]
pub fn validate_windows_volume(
    config: &AccountsConfig,
    target: &Path,
    image_bytes: u64,
) -> Result<WindowsVolumeInfo, OperationError> {
    let info = sanikey_windows_volume::inspect(target)
        .map_err(|error| usb_error(format!("lettura del volume Windows fallita: {error}")))?;
    validate_windows_volume_info(
        config,
        info.identity.as_str(),
        &info.filesystem,
        info.available_bytes,
        image_bytes,
    )?;
    Ok(info)
}

#[cfg(any(windows, test))]
fn validate_windows_volume_info(
    config: &AccountsConfig,
    volume_guid_path: &str,
    filesystem: &str,
    available_bytes: u64,
    image_bytes: u64,
) -> Result<(), OperationError> {
    let reserve_bytes = u64::try_from(config.usb.min_free_space_mb)
        .ok()
        .and_then(|megabytes| megabytes.checked_mul(1024 * 1024))
        .ok_or_else(|| usb_error("margine minimo dello spazio USB non valido"))?;
    let required_bytes = image_bytes
        .checked_add(reserve_bytes)
        .ok_or_else(|| usb_error("spazio richiesto per l'export USB fuori intervallo"))?;
    if available_bytes < required_bytes {
        return Err(usb_error(
            "target USB Windows con spazio libero insufficiente",
        ));
    }

    if let Some(expected) = expected_windows_volume_guid(config)?
        && expected != volume_guid_path
    {
        return Err(usb_error(
            "Volume GUID del target USB Windows non corrispondente",
        ));
    }
    if config.usb.require_exfat && !filesystem.eq_ignore_ascii_case("exFAT") {
        return Err(usb_error(
            "il filesystem del volume USB Windows deve essere exFAT",
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn expected_windows_volume_guid(config: &AccountsConfig) -> Result<Option<&str>, OperationError> {
    if let Some(volume_guid_path) = config.usb.usb_uuid.as_deref() {
        return Ok(Some(volume_guid_path));
    }
    let mut configured = config
        .enabled_people()
        .map(|person| person.usb_uuid.as_str())
        .filter(|value| !value.is_empty());
    let Some(first) = configured.next() else {
        return Ok(None);
    };
    if configured.any(|value| value != first) {
        return Err(usb_error(
            "le persone abilitate configurano Volume GUID Windows differenti",
        ));
    }
    Ok(Some(first))
}

#[cfg(any(windows, test))]
fn usb_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Usb, message)
}

#[cfg(test)]
mod tests {
    use super::{expected_windows_volume_guid, validate_windows_volume_info};
    use crate::config::{AccountsConfig, PersonConfig, UsbConfig};
    use std::path::PathBuf;

    const GUID: &str = r"\\?\Volume{SYNTHETIC-GUID}\";

    fn config(global_guid: Option<&str>, people: &[(&str, bool)]) -> AccountsConfig {
        AccountsConfig {
            config_version: 1,
            people: people
                .iter()
                .map(|(guid, enabled)| PersonConfig {
                    id: "fixture-person".to_owned(),
                    display_name: "Persona sintetica".to_owned(),
                    source_documents: PathBuf::new(),
                    metadata_directory: PathBuf::new(),
                    local_build: PathBuf::new(),
                    usb_uuid: (*guid).to_owned(),
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
                usb_uuid: global_guid.map(str::to_owned),
                require_exfat: false,
                min_free_space_mb: 1,
                copy_strategy: "python".to_owned(),
            },
        }
    }

    #[test]
    fn global_windows_volume_guid_takes_precedence_and_is_compared_literally() {
        let configured = config(Some(GUID), &[("OTHER-GUID", true)]);
        assert_eq!(
            expected_windows_volume_guid(&configured).unwrap(),
            Some(GUID)
        );
        assert!(
            validate_windows_volume_info(&configured, GUID, "NTFS", 2 * 1024 * 1024, 1024,).is_ok()
        );
        assert!(
            validate_windows_volume_info(
                &configured,
                GUID.to_ascii_lowercase().as_str(),
                "NTFS",
                2 * 1024 * 1024,
                1024,
            )
            .is_err()
        );
    }

    #[test]
    fn shared_enabled_person_volume_guid_is_used_when_no_global_value_exists() {
        let configured = config(None, &[(GUID, true), (GUID, true), ("DISABLED", false)]);
        assert_eq!(
            expected_windows_volume_guid(&configured).unwrap(),
            Some(GUID)
        );
        assert!(
            validate_windows_volume_info(&configured, GUID, "exFAT", 2 * 1024 * 1024, 1024,)
                .is_ok()
        );
    }

    #[test]
    fn rejects_windows_volume_mismatch_filesystem_and_insufficient_space() {
        let mut configured = config(Some(GUID), &[]);
        configured.usb.require_exfat = true;
        assert!(
            validate_windows_volume_info(
                &configured,
                "\\?\\Volume{OTHER-GUID}\\",
                "exFAT",
                2 * 1024 * 1024,
                1024,
            )
            .is_err()
        );
        assert!(
            validate_windows_volume_info(&configured, GUID, "NTFS", 2 * 1024 * 1024, 1024,)
                .is_err()
        );
        assert!(
            validate_windows_volume_info(
                &configured,
                GUID,
                "exFAT",
                2 * 1024 * 1024,
                2 * 1024 * 1024,
            )
            .is_err()
        );
    }
}
