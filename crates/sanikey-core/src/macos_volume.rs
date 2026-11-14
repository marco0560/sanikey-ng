//! Controllo del volume USB macOS e del suo VolumeUUID nativo.

#[cfg(any(target_os = "macos", test))]
use crate::config::AccountsConfig;
#[cfg(any(target_os = "macos", test))]
use crate::config::trace::trace_accounts_paths;
#[cfg(any(target_os = "macos", test))]
use crate::logging::TraceOmission;
#[cfg(any(target_os = "macos", test))]
use crate::logging::{Component, TraceField, TraceValue, trace_current, trace_sensitive_text};
#[cfg(any(target_os = "macos", test))]
use crate::operation::{ErrorCode, OperationError};
#[cfg(target_os = "macos")]
use sanikey_macos_volume::MacVolumeInfo;
#[cfg(any(target_os = "macos", test))]
use sanikey_macos_volume::MacVolumeObservation;
#[cfg(any(target_os = "macos", test))]
use serde_json::json;
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
    let trace = trace_current(
        Component::Core,
        "macos_volume::validate_macos_volume",
        || {
            vec![
                TraceField::new("config", trace_volume_config(config)),
                TraceField::new("target", TraceValue::path(target)),
                TraceField::new("image_bytes", TraceValue::Data(json!(image_bytes))),
                TraceField::new(
                    "external_component_internals",
                    TraceValue::Omitted(TraceOmission::ExternalComponent),
                ),
            ]
        },
    );
    let result = (|| {
        let info = sanikey_macos_volume::inspect_with_observer(target, trace_macos_observation)
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
    })();
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(info) => json!({
                "outcome": "succeeded",
                "mount_point": TraceValue::path(&info.mount_point).to_json(),
                "volume_uuid": info.identity.as_ref().map(sanikey_macos_volume::MacVolumeUuid::as_str),
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

#[cfg(any(target_os = "macos", test))]
fn trace_macos_observation(observation: MacVolumeObservation) {
    let (call, fields) = match observation {
        MacVolumeObservation::Statfs {
            target,
            duration_micros,
            success,
            mount_point,
            filesystem,
            available_bytes,
            error_code,
        } => (
            "macos_volume::ffi::statfs",
            vec![
                TraceField::new("target", TraceValue::path(&target)),
                TraceField::new(
                    "measured_duration_micros",
                    TraceValue::data(duration_micros),
                ),
                TraceField::new("success", TraceValue::data(success)),
                TraceField::new(
                    "mount_point",
                    mount_point.map_or_else(
                        || TraceValue::Data(json!(null)),
                        |path| TraceValue::path(&path),
                    ),
                ),
                TraceField::new("filesystem", TraceValue::data(filesystem)),
                TraceField::new("available_bytes", TraceValue::data(available_bytes)),
                TraceField::new("error_code", TraceValue::data(error_code)),
                TraceField::new(
                    "raw_statfs_buffers",
                    TraceValue::Omitted(TraceOmission::UnsupportedType),
                ),
            ],
        ),
        MacVolumeObservation::Diskutil {
            mount_point,
            duration_micros,
            success,
            exit_code,
            stdout,
            stderr,
            error_code,
        } => (
            "macos_volume::diskutil_info",
            vec![
                TraceField::new("program", TraceValue::data("/usr/sbin/diskutil")),
                TraceField::new("arguments", TraceValue::data(json!(["info", "-plist"]))),
                TraceField::new("mount_point", TraceValue::path(&mount_point)),
                TraceField::new(
                    "measured_duration_micros",
                    TraceValue::data(duration_micros),
                ),
                TraceField::new("success", TraceValue::data(success)),
                TraceField::new("exit_code", TraceValue::data(exit_code)),
                TraceField::new("error_code", TraceValue::data(error_code)),
                TraceField::new(
                    "stdout",
                    trace_sensitive_text(&String::from_utf8_lossy(&stdout)),
                ),
                TraceField::new(
                    "stderr",
                    trace_sensitive_text(&String::from_utf8_lossy(&stderr)),
                ),
                TraceField::new("stdout_byte_count", TraceValue::data(stdout.len())),
                TraceField::new("stderr_byte_count", TraceValue::data(stderr.len())),
                TraceField::new(
                    "raw_process_output_bytes",
                    TraceValue::Omitted(TraceOmission::ExternalComponent),
                ),
            ],
        ),
        MacVolumeObservation::PlistParse {
            xml,
            duration_micros,
            success,
            volume_uuid,
            error_kind,
            error_message,
        } => {
            let xml_byte_count = xml.len();
            let xml_text = String::from_utf8(xml)
                .map(|xml| trace_sensitive_text(&xml))
                .unwrap_or(TraceValue::Omitted(TraceOmission::UnsupportedType));
            let result = json!({
                "success": success,
                "volume_uuid": volume_uuid.as_deref().map(trace_sensitive_text).map(|value| value.to_json()),
                "error_kind": error_kind.map(|kind| format!("{kind:?}")),
                "error_message": error_message.as_deref().map(trace_sensitive_text).map(|value| value.to_json()),
            });
            let trace = trace_current(
                Component::Core,
                "macos_volume::parse_volume_uuid_plist",
                || {
                    vec![
                        TraceField::new("xml_byte_count", TraceValue::data(xml_byte_count as u64)),
                        TraceField::new("xml", xml_text),
                        TraceField::new(
                            "measured_duration_micros",
                            TraceValue::data(duration_micros),
                        ),
                    ]
                },
            );
            if let Some(trace) = trace {
                let _ = trace.finish(TraceValue::Data(result));
            }
            return;
        }
    };
    let trace = trace_current(Component::Core, call, || fields);
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({"observed":true})));
    }
}

#[cfg(any(target_os = "macos", test))]
fn trace_volume_config(config: &AccountsConfig) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("format", TraceValue::data("rust_debug")),
        TraceField::new("value", trace_sensitive_text(&format!("{config:#?}"))),
        TraceField::new("native_paths", trace_accounts_paths(config)),
    ])
}

#[cfg(any(target_os = "macos", test))]
fn validate_volume_info(
    config: &AccountsConfig,
    volume_uuid: Option<&str>,
    filesystem: &str,
    available_bytes: u64,
    image_bytes: u64,
) -> Result<(), OperationError> {
    let trace = trace_current(
        Component::Core,
        "macos_volume::validate_volume_info",
        || {
            vec![
                TraceField::new("config", trace_volume_config(config)),
                TraceField::new(
                    "volume_uuid",
                    volume_uuid.map_or_else(
                        || TraceValue::Data(json!(null)),
                        |uuid| TraceValue::Data(json!(uuid)),
                    ),
                ),
                TraceField::new("filesystem", TraceValue::Data(json!(filesystem))),
                TraceField::new("available_bytes", TraceValue::Data(json!(available_bytes))),
                TraceField::new("image_bytes", TraceValue::Data(json!(image_bytes))),
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

#[cfg(any(target_os = "macos", test))]
fn expected_volume_uuid(config: &AccountsConfig) -> Result<Option<&str>, OperationError> {
    let trace = trace_current(
        Component::Core,
        "macos_volume::expected_volume_uuid",
        || vec![TraceField::new("config", trace_volume_config(config))],
    );
    let result = (|| {
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
    })();
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(Some(uuid)) => json!({"outcome": "succeeded", "volume_uuid": uuid}),
            Ok(None) => json!({"outcome": "succeeded", "volume_uuid": null}),
            Err(error) => {
                json!({"outcome": "failed", "code": error.code().as_str(), "message": trace_sensitive_text(error.message()).to_json()})
            }
        };
        let _ = trace.finish(TraceValue::Data(result_value));
    }
    result
}

#[cfg(any(target_os = "macos", test))]
fn usb_error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    let trace = trace_current(Component::Core, "macos_volume::usb_error", || {
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

    #[test]
    // Le fixture Linux esercitano solo i controlli puri e non qualificano statfs o diskutil su macOS.
    fn synthetic_logging_separates_trace_details_from_events() {
        use super::trace_macos_observation;
        use crate::logging::{Component, Level, Session};
        use sanikey_macos_volume::MacVolumeObservation;
        use std::fs;
        use std::path::PathBuf;

        for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
            let directory = std::env::temp_dir().join(format!(
                "sanikey-macos-volume-{suffix}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let (session, path) =
                Session::create_in_directory_with_level(&directory, Component::Core, level)
                    .expect("sessione sintetica");
            let mut configured = config(Some(UUID), &[(UUID, true)]);
            configured.people[0].display_name =
                "api_key = fixture-macos-volume-config-secret".to_owned();
            session.scoped(|| {
                assert_eq!(expected_volume_uuid(&configured).unwrap(), Some(UUID));
                validate_volume_info(&configured, Some(UUID), "exFAT", 2 * 1024 * 1024, 1024)
                    .expect("metadati sintetici validi");
                trace_macos_observation(MacVolumeObservation::Statfs {
                    target: PathBuf::from("/synthetic/usb"),
                    duration_micros: 101,
                    success: true,
                    mount_point: Some(PathBuf::from("/Volumes/Synthetic")),
                    filesystem: Some("exFAT".to_owned()),
                    available_bytes: Some(1024 * 1024),
                    error_code: None,
                });
                trace_macos_observation(MacVolumeObservation::Diskutil {
                    mount_point: PathBuf::from("/Volumes/Synthetic"),
                    duration_micros: 202,
                    success: true,
                    exit_code: Some(0),
                    stdout: b"<plist><dict><key>VolumeUUID</key><string>SYNTHETIC-UUID</string><key>VolumeName</key><string>private-volume-label</string><key>api_key</key><string>fixture-diskutil-secret</string></dict></plist>".to_vec(),
                    stderr: Vec::new(),
                    error_code: None,
                });
                trace_macos_observation(MacVolumeObservation::PlistParse {
                    xml: b"<plist><dict><key>VolumeUUID</key><string>SYNTHETIC-PARSER-UUID</string><key>VolumeName</key><string>private-parser-volume-label</string><key>api_key</key><string>fixture-parser-secret</string></dict></plist>".to_vec(),
                    duration_micros: 303,
                    success: true,
                    volume_uuid: Some("SYNTHETIC-PARSER-UUID".to_owned()),
                    error_kind: None,
                    error_message: None,
                });
                trace_macos_observation(MacVolumeObservation::PlistParse {
                    xml: b"<plist><dict><key>api_key</key><string>fixture-parser-failure-secret</string></dict>".to_vec(),
                    duration_micros: 404,
                    success: false,
                    volume_uuid: None,
                    error_kind: Some(std::io::ErrorKind::InvalidData),
                    error_message: Some(
                        "plist non valido: api_key = \"fixture-parser-failure-secret\""
                            .to_owned(),
                    ),
                });
                assert!(
                    validate_volume_info(
                        &configured,
                        Some("OTHER-SYNTHETIC-UUID"),
                        "exFAT",
                        2 * 1024 * 1024,
                        1024,
                    )
                    .is_err()
                );
            });
            drop(session);

            let contents = fs::read_to_string(&path).expect("log sintetico");
            if level == Level::Trace {
                assert!(contents.contains("macos_volume::expected_volume_uuid"));
                assert!(contents.contains("macos_volume::validate_volume_info"));
                assert!(contents.contains("macos_volume::usb_error"));
                assert!(contents.contains("macos_volume::ffi::statfs"));
                assert!(contents.contains("macos_volume::diskutil_info"));
                assert!(contents.contains("macos_volume::parse_volume_uuid_plist"));
                assert!(contents.contains("stdout"));
                assert!(contents.contains("stdout_byte_count"));
                assert!(contents.contains("measured_duration_micros"));
                for duration in [101, 202, 303, 404] {
                    assert!(logged_duration(&contents, duration));
                }
                assert!(contents.contains("raw_process_output_bytes"));
                assert!(contents.contains("raw_statfs_buffers"));
                assert!(contents.contains("SYNTHETIC-UUID"));
                assert!(contents.contains("private-volume-label"));
                assert!(contents.contains("private-parser-volume-label"));
                assert!(contents.contains("secret_omitted"));
                assert!(!contents.contains("fixture-diskutil-secret"));
                assert!(!contents.contains("fixture-parser-secret"));
                assert!(!contents.contains("fixture-parser-failure-secret"));
                assert!(!contents.contains("fixture-macos-volume-config-secret"));
                assert!(contents.contains("config_version"));
                assert!(contents.contains("display_name"));
                assert!(contents.contains("ABCD-1234-SYNTHETIC"));
            } else {
                assert!(!contents.contains("function_enter"));
                assert!(!contents.contains("ABCD-1234-SYNTHETIC"));
                assert!(!contents.contains("SYNTHETIC-UUID"));
                assert!(!contents.contains("SYNTHETIC-PARSER-UUID"));
                assert!(!contents.contains("private-parser-volume-label"));
                assert!(!contents.contains("fixture-parser-secret"));
                assert!(!contents.contains("fixture-parser-failure-secret"));
                assert!(!contents.contains("fixture-macos-volume-config-secret"));
                assert!(!contents.contains("Persona sintetica"));
            }
            fs::remove_file(path).expect("rimozione log sintetico");
            fs::remove_dir_all(directory).expect("rimozione directory sintetica");
        }
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
}
