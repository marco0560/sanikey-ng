//! Controllo del volume USB macOS e del suo VolumeUUID nativo.

#[cfg(any(target_os = "macos", test))]
use crate::config::AccountsConfig;
#[cfg(any(target_os = "macos", test))]
use crate::operation::{ErrorCode, OperationError};
#[cfg(target_os = "macos")]
use sanikey_macos_volume::MacVolumeInfo;
#[cfg(target_os = "macos")]
use std::path::Path;

/// Legge e convalida il volume macOS configurato che contiene il target.
///
/// `usb_uuid` contiene il VolumeUUID nativo macOS ed è confrontato
/// letteralmente. Il controllo non crea directory né scrive sul volume.
#[cfg(target_os = "macos")]
pub fn validate_macos_volume(
    config: &AccountsConfig,
    target: &Path,
    image_bytes: u64,
) -> Result<MacVolumeInfo, OperationError> {
    let info = sanikey_macos_volume::inspect(target)
        .map_err(|error| usb_error(format!("lettura del volume USB macOS fallita: {error}")))?;
    validate_volume_info(
        config,
        info.identity
            .as_ref()
            .map(sanikey_macos_volume::MacVolumeUuid::as_str),
        &info.filesystem,
        info.available_bytes,
        image_bytes,
    )?;
    Ok(info)
}

#[cfg(any(target_os = "macos", test))]
fn validate_volume_info(
    config: &AccountsConfig,
    volume_uuid: Option<&str>,
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
            "target USB macOS con spazio libero insufficiente",
        ));
    }
    if let Some(expected) = expected_volume_uuid(config)?
        && volume_uuid != Some(expected)
    {
        return Err(usb_error("VolumeUUID macOS del target non corrispondente"));
    }
    if config.usb.require_exfat && !filesystem.eq_ignore_ascii_case("exfat") {
        return Err(usb_error(
            "il filesystem del volume USB macOS deve essere exFAT",
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn expected_volume_uuid(config: &AccountsConfig) -> Result<Option<&str>, OperationError> {
    if let Some(volume_uuid) = config.usb.usb_uuid.as_deref() {
        return Ok(Some(volume_uuid));
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
            "le persone abilitate configurano VolumeUUID macOS differenti",
        ));
    }
    Ok(Some(first))
}

#[cfg(any(target_os = "macos", test))]
fn usb_error(message: impl Into<String>) -> OperationError {
    OperationError::new(ErrorCode::Usb, message)
}

#[cfg(test)]
mod tests {
    use super::{expected_volume_uuid, validate_volume_info};
    use crate::config::{AccountsConfig, PersonConfig, UsbConfig};
    use std::path::PathBuf;

    const UUID: &str = "ABCD-1234-SYNTHETIC";

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

    #[test]
    fn global_volume_uuid_takes_precedence_and_is_compared_literally() {
        let configured = config(Some(UUID), &[("OTHER-UUID", true)]);
        assert_eq!(expected_volume_uuid(&configured).unwrap(), Some(UUID));
        assert!(
            validate_volume_info(&configured, Some(UUID), "APFS", 2 * 1024 * 1024, 1024).is_ok()
        );
        assert!(
            validate_volume_info(
                &configured,
                Some("abcd-1234-synthetic"),
                "APFS",
                2 * 1024 * 1024,
                1024,
            )
            .is_err()
        );
    }

    #[test]
    fn shared_enabled_person_uuid_is_used_when_global_value_is_absent() {
        let configured = config(None, &[(UUID, true), (UUID, true), ("DISABLED", false)]);
        assert_eq!(expected_volume_uuid(&configured).unwrap(), Some(UUID));
    }

    #[test]
    fn rejects_uuid_mismatch_filesystem_and_insufficient_space() {
        let mut configured = config(Some(UUID), &[]);
        configured.usb.require_exfat = true;
        assert!(
            validate_volume_info(
                &configured,
                Some("OTHER-UUID"),
                "exFAT",
                2 * 1024 * 1024,
                1024
            )
            .is_err()
        );
        assert!(
            validate_volume_info(&configured, Some(UUID), "APFS", 2 * 1024 * 1024, 1024).is_err()
        );
        assert!(
            validate_volume_info(
                &configured,
                Some(UUID),
                "exFAT",
                2 * 1024 * 1024,
                2 * 1024 * 1024
            )
            .is_err()
        );
        assert!(validate_volume_info(&configured, None, "exFAT", 2 * 1024 * 1024, 1024).is_err());
    }

    #[test]
    fn missing_volume_uuid_is_allowed_only_when_configuration_does_not_require_it() {
        let configured = config(None, &[]);
        assert!(validate_volume_info(&configured, None, "APFS", 2 * 1024 * 1024, 1024).is_ok());
        let configured = config(None, &[(UUID, true)]);
        assert!(validate_volume_info(&configured, None, "APFS", 2 * 1024 * 1024, 1024).is_err());
    }
}
