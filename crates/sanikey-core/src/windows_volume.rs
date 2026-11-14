//! Controllo Windows del volume configurato, senza euristiche sui mount Linux.

#[cfg(any(windows, test))]
use crate::config::AccountsConfig;
#[cfg(any(windows, test))]
use crate::config::trace::trace_accounts_paths;
#[cfg(any(windows, test))]
use crate::logging::{Component, TraceField, TraceValue, trace_current, trace_sensitive_text};
#[cfg(any(windows, test))]
use crate::operation::{ErrorCode, OperationError};
#[cfg(windows)]
use sanikey_windows_volume::WindowsVolumeInfo;
#[cfg(any(windows, test))]
use sanikey_windows_volume::WindowsVolumeObservation;
#[cfg(any(windows, test))]
use serde_json::json;
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
    let trace = trace_current(
        Component::Core,
        "windows_volume::validate_windows_volume",
        || {
            vec![
                TraceField::new("target", TraceValue::path(target)),
                TraceField::new("image_bytes", TraceValue::Data(json!(image_bytes))),
                TraceField::new("config", trace_volume_config(config)),
            ]
        },
    );
    let result = (|| {
        let info = sanikey_windows_volume::inspect_with_observer(target, trace_windows_observation)
            .map_err(|error| usb_error(format!("lettura del volume Windows fallita: {error}")))?;
        validate_windows_volume_info(
            config,
            info.identity.as_str(),
            &info.filesystem,
            info.available_bytes,
            image_bytes,
        )?;
        Ok(info)
    })();
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(info) => json!({
                "outcome": "succeeded",
                "mount_path": TraceValue::path(&info.mount_path).to_json(),
                "identity": info.identity.as_str(),
                "filesystem": info.filesystem,
                "available_bytes": info.available_bytes,
            }),
            Err(error) => {
                json!({"outcome": "failed", "code": error.code().as_str(), "message": trace_sensitive_text(error.message()).to_json()})
            }
        };
        let _ = trace.finish(TraceValue::Data(result_value));
    }
    result
}

#[cfg(any(windows, test))]
fn trace_windows_observation(observation: WindowsVolumeObservation) {
    let (call, fields) = match observation {
        WindowsVolumeObservation::VolumePath {
            target,
            duration_micros,
            success,
            mount_path,
            error_code,
        } => (
            "windows_volume::ffi::GetVolumePathNameW",
            vec![
                TraceField::new("target", TraceValue::path(&target)),
                TraceField::new(
                    "measured_duration_micros",
                    TraceValue::data(duration_micros),
                ),
                TraceField::new(
                    "mount_path",
                    mount_path.map_or_else(
                        || TraceValue::Data(json!(null)),
                        |path| TraceValue::path(&path),
                    ),
                ),
                TraceField::new("success", TraceValue::data(success)),
                TraceField::new("error_code", TraceValue::data(error_code)),
            ],
        ),
        WindowsVolumeObservation::VolumeGuid {
            mount_path,
            duration_micros,
            success,
            volume_guid_path,
            error_code,
        } => (
            "windows_volume::ffi::GetVolumeNameForVolumeMountPointW",
            vec![
                TraceField::new("mount_path", TraceValue::path(&mount_path)),
                TraceField::new(
                    "measured_duration_micros",
                    TraceValue::data(duration_micros),
                ),
                TraceField::new("success", TraceValue::data(success)),
                TraceField::new("volume_guid_path", TraceValue::data(volume_guid_path)),
                TraceField::new("error_code", TraceValue::data(error_code)),
            ],
        ),
        WindowsVolumeObservation::Filesystem {
            mount_path,
            duration_micros,
            success,
            filesystem,
            error_code,
        } => (
            "windows_volume::ffi::GetVolumeInformationW",
            vec![
                TraceField::new("mount_path", TraceValue::path(&mount_path)),
                TraceField::new(
                    "measured_duration_micros",
                    TraceValue::data(duration_micros),
                ),
                TraceField::new("success", TraceValue::data(success)),
                TraceField::new("filesystem", TraceValue::data(filesystem)),
                TraceField::new("error_code", TraceValue::data(error_code)),
            ],
        ),
        WindowsVolumeObservation::AvailableSpace {
            target,
            duration_micros,
            success,
            available_bytes,
            error_code,
        } => (
            "windows_volume::ffi::GetDiskFreeSpaceExW",
            vec![
                TraceField::new("target", TraceValue::path(&target)),
                TraceField::new(
                    "measured_duration_micros",
                    TraceValue::data(duration_micros),
                ),
                TraceField::new("success", TraceValue::data(success)),
                TraceField::new("available_bytes", TraceValue::data(available_bytes)),
                TraceField::new("error_code", TraceValue::data(error_code)),
            ],
        ),
    };
    let trace = trace_current(Component::Core, call, || {
        let mut fields = fields;
        fields.push(TraceField::new(
            "native_buffer_contents",
            TraceValue::Omitted(crate::logging::TraceOmission::UnsupportedType),
        ));
        fields
    });
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({"observed":true})));
    }
}

#[cfg(any(windows, test))]
fn trace_volume_config(config: &AccountsConfig) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("format", TraceValue::data("rust_debug")),
        TraceField::new("value", trace_sensitive_text(&format!("{config:#?}"))),
        TraceField::new("native_paths", trace_accounts_paths(config)),
    ])
}

#[cfg(any(windows, test))]
fn validate_windows_volume_info(
    config: &AccountsConfig,
    volume_guid_path: &str,
    filesystem: &str,
    available_bytes: u64,
    image_bytes: u64,
) -> Result<(), OperationError> {
    let trace = trace_current(
        Component::Core,
        "windows_volume::validate_windows_volume_info",
        || {
            vec![
                TraceField::new(
                    "volume_guid_path",
                    TraceValue::Data(json!(volume_guid_path)),
                ),
                TraceField::new("filesystem", TraceValue::Data(json!(filesystem))),
                TraceField::new("available_bytes", TraceValue::Data(json!(available_bytes))),
                TraceField::new("image_bytes", TraceValue::Data(json!(image_bytes))),
                TraceField::new("config", trace_volume_config(config)),
            ]
        },
    );
    let result = (|| {
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
    })();
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(()) => json!({"outcome": "succeeded"}),
            Err(error) => {
                json!({"outcome": "failed", "code": error.code().as_str(), "message": trace_sensitive_text(error.message()).to_json()})
            }
        };
        let _ = trace.finish(TraceValue::Data(result_value));
    }
    result
}

#[cfg(any(windows, test))]
fn expected_windows_volume_guid(config: &AccountsConfig) -> Result<Option<&str>, OperationError> {
    let trace = trace_current(
        Component::Core,
        "windows_volume::expected_windows_volume_guid",
        || vec![TraceField::new("config", trace_volume_config(config))],
    );
    let result = (|| {
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
    })();
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(Some(guid)) => json!({"outcome": "succeeded", "volume_guid_path": guid}),
            Ok(None) => json!({"outcome": "succeeded", "volume_guid_path": null}),
            Err(error) => {
                json!({"outcome": "failed", "code": error.code().as_str(), "message": trace_sensitive_text(error.message()).to_json()})
            }
        };
        let _ = trace.finish(TraceValue::Data(result_value));
    }
    result
}

#[cfg(any(windows, test))]
fn usb_error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    let trace = trace_current(Component::Core, "windows_volume::usb_error", || {
        vec![TraceField::new("message", trace_sensitive_text(&message))]
    });
    let error = OperationError::new(ErrorCode::Usb, message);
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(
            json!({"code": error.code().as_str(), "message": trace_sensitive_text(error.message()).to_json()}),
        ));
    }
    error
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

    #[test]
    // Questo test Linux copre solo la validazione pura con fixture sintetiche; non esercita le API Win32.
    fn synthetic_linux_trace_records_volume_checks_without_qualifying_win32() {
        use super::trace_windows_observation;
        use crate::logging::{Component, Level, Session};
        use sanikey_windows_volume::WindowsVolumeObservation;
        use std::fs;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-windows-volume-trace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione sintetica");
        let mut configured = config(Some(GUID), &[(GUID, true)]);
        configured.people[0].display_name =
            "api_key = fixture-windows-volume-config-secret".to_owned();
        session.scoped(|| {
            assert_eq!(
                expected_windows_volume_guid(&configured).unwrap(),
                Some(GUID)
            );
            validate_windows_volume_info(&configured, GUID, "exFAT", 2 * 1024 * 1024, 1024)
                .expect("metadati sintetici validi");
            trace_windows_observation(WindowsVolumeObservation::VolumePath {
                target: PathBuf::from("/synthetic/usb"),
                duration_micros: 101,
                success: true,
                mount_path: Some(PathBuf::from("R:\\")),
                error_code: None,
            });
            trace_windows_observation(WindowsVolumeObservation::VolumeGuid {
                mount_path: PathBuf::from("R:\\"),
                duration_micros: 202,
                success: true,
                volume_guid_path: Some(GUID.to_owned()),
                error_code: None,
            });
            trace_windows_observation(WindowsVolumeObservation::Filesystem {
                mount_path: PathBuf::from("R:\\"),
                duration_micros: 303,
                success: true,
                filesystem: Some("exFAT".to_owned()),
                error_code: None,
            });
            trace_windows_observation(WindowsVolumeObservation::AvailableSpace {
                target: PathBuf::from("/synthetic/usb"),
                duration_micros: 404,
                success: true,
                available_bytes: Some(1024 * 1024),
                error_code: None,
            });
        });
        drop(session);

        let contents = fs::read_to_string(&path).expect("log sintetico");
        assert!(contents.contains("windows_volume::expected_windows_volume_guid"));
        assert!(contents.contains("windows_volume::validate_windows_volume_info"));
        assert!(contents.contains("windows_volume::ffi::GetVolumePathNameW"));
        assert!(contents.contains("windows_volume::ffi::GetVolumeNameForVolumeMountPointW"));
        assert!(contents.contains("windows_volume::ffi::GetVolumeInformationW"));
        assert!(contents.contains("windows_volume::ffi::GetDiskFreeSpaceExW"));
        assert!(contents.contains("native_buffer_contents"));
        for duration in [101, 202, 303, 404] {
            assert!(logged_duration(&contents, duration));
        }
        assert!(contents.contains("SYNTHETIC-GUID"));
        assert!(contents.contains("config_version"));
        assert!(contents.contains("display_name"));
        assert!(contents.contains("display_name"));
        assert!(contents.contains("source_documents"));
        assert!(contents.contains("copy_strategy"));
        assert!(contents.contains("secret_omitted"));
        assert!(!contents.contains("fixture-windows-volume-config-secret"));
        fs::remove_file(path).expect("rimozione log sintetico");
        fs::remove_dir(directory).expect("rimozione directory sintetica");
    }

    #[cfg(test)]
    fn logged_duration(contents: &str, duration: u64) -> bool {
        contents.lines().any(|line| {
            let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
                return false;
            };
            record["type"] == "function_enter"
                && record["arguments"].as_array().is_some_and(|arguments| {
                    arguments.iter().any(|argument| {
                        argument["name"] == "measured_duration_micros"
                            && argument["content"]["value"] == duration
                    })
                })
        })
    }

    #[cfg(unix)]
    #[test]
    fn trace_path_roundtrips_non_utf8_unix_bytes_without_loss() {
        use crate::logging::{Component, Level, Session, TraceField, TraceValue, trace_current};
        use serde_json::json;
        use std::ffi::OsString;
        use std::fs;
        use std::os::unix::ffi::OsStringExt;

        let directory = std::env::temp_dir().join(format!(
            "sanikey-windows-volume-path-trace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione sintetica");
        let original = PathBuf::from(OsString::from_vec(vec![b't', 0xff, b'g']));
        session.scoped(|| {
            let span = trace_current(Component::Core, "windows_volume::path_fixture", || {
                vec![TraceField::new("target", TraceValue::path(&original))]
            })
            .expect("traccia attiva");
            span.finish(TraceValue::Data(json!({"outcome": "succeeded"})))
                .expect("uscita sintetica");
        });
        drop(session);

        let contents = fs::read_to_string(&path).expect("log sintetico");
        let entry: serde_json::Value = contents
            .lines()
            .map(|line| -> serde_json::Value { serde_json::from_str(line).expect("record JSON") })
            .find(|entry| entry["type"] == "function_enter")
            .expect("ingresso sintetico");
        let value = &entry["arguments"][0]["content"]["value"];
        assert_eq!(value["encoding"], "unix_bytes");
        let bytes = value["value"]
            .as_array()
            .expect("byte codificati")
            .iter()
            .map(|byte| {
                u8::try_from(byte.as_u64().expect("byte numerico")).expect("byte nell'intervallo")
            })
            .collect::<Vec<_>>();
        assert_eq!(PathBuf::from(OsString::from_vec(bytes)), original);
        fs::remove_file(path).expect("rimozione log sintetico");
        fs::remove_dir(directory).expect("rimozione directory sintetica");
    }
}
