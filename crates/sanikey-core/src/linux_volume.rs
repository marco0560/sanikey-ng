//! Lettura e controllo dei metadati di un volume Linux prima della copia USB.

use crate::config::AccountsConfig;
use crate::logging::{
    Component, TraceField, TraceOmission, TraceValue, trace_current, trace_json_value,
    trace_sensitive_text,
};
use crate::operation::{ErrorCode, OperationError};
use rustix::fs::{StatVfs, statvfs};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
    let trace = trace_current(
        Component::Core,
        "linux_volume::inspect_linux_volume",
        || vec![TraceField::new("target", TraceValue::path(target))],
    );
    let mut statvfs_trace = None;
    let result = (|| {
        if !target.is_dir() {
            return Err(usb_error(
                "il target Linux deve essere una directory esistente",
            ));
        }

        let statistics_result = statvfs(target);
        if trace.is_some() {
            statvfs_trace = Some(trace_statvfs_result(&statistics_result));
        }
        let statistics = statistics_result.map_err(|error| {
            usb_error(format!("lettura spazio del volume Linux fallita: {error}"))
        })?;
        let available_bytes = calculate_available_bytes(&statistics)?;

        let mount = find_mount_info(target, || {
            Command::new("findmnt")
                .args(["--json", "--output", "TARGET,FSTYPE,UUID", "--target"])
                .arg(target)
                .output()
        });
        Ok(LinuxVolumeInfo {
            mount_target: mount
                .as_ref()
                .and_then(|info| info.get("target"))
                .map(PathBuf::from),
            filesystem: mount.as_ref().and_then(|info| info.get("fstype")).cloned(),
            uuid: mount.and_then(|info| info.get("uuid").cloned()),
            available_bytes,
        })
    })();
    if let Some(trace) = trace {
        let statvfs_trace = statvfs_trace
            .unwrap_or_else(|| json!({"outcome":"not_called","reason":"target_not_directory"}));
        let result_value = match &result {
            Ok(info) => json!({
                "outcome": "succeeded",
                "statvfs_result": statvfs_trace,
                "mount_target": info.mount_target.as_ref().map(|path| TraceValue::path(path).to_json()),
                "filesystem": info.filesystem,
                "uuid": info.uuid,
                "available_bytes": info.available_bytes,
            }),
            Err(error) => {
                json!({"outcome": "failed", "code": error.code().as_str(), "message": trace_sensitive_text(error.message()).to_json(), "statvfs_result": statvfs_trace})
            }
        };
        let _ = trace.finish(TraceValue::Data(result_value));
    }
    result
}

fn trace_statvfs_result(result: &rustix::io::Result<StatVfs>) -> serde_json::Value {
    match result {
        Ok(statistics) => json!({
            "outcome": "succeeded",
            "f_bsize": statistics.f_bsize,
            "f_frsize": statistics.f_frsize,
            "f_blocks": statistics.f_blocks,
            "f_bfree": statistics.f_bfree,
            "f_bavail": statistics.f_bavail,
            "f_files": statistics.f_files,
            "f_ffree": statistics.f_ffree,
            "f_favail": statistics.f_favail,
            "f_fsid": statistics.f_fsid,
            "f_flag_bits": statistics.f_flag.bits(),
            "f_namemax": statistics.f_namemax,
        }),
        Err(error) => json!({
            "outcome": "failed",
            "error": trace_sensitive_text(&error.to_string()).to_json(),
        }),
    }
}

fn calculate_available_bytes(statistics: &StatVfs) -> Result<u64, OperationError> {
    let trace = trace_current(
        Component::Core,
        "linux_volume::calculate_available_bytes",
        || {
            vec![
                TraceField::new("f_bfree", TraceValue::Data(json!(statistics.f_bfree))),
                TraceField::new("f_frsize", TraceValue::Data(json!(statistics.f_frsize))),
                TraceField::new("f_bsize", TraceValue::Data(json!(statistics.f_bsize))),
            ]
        },
    );
    let block_size = if statistics.f_frsize == 0 {
        statistics.f_bsize
    } else {
        statistics.f_frsize
    };
    let result = statistics
        .f_bfree
        .checked_mul(block_size)
        .ok_or_else(|| usb_error("spazio disponibile Linux fuori intervallo"));
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(available_bytes) => json!({"outcome":"succeeded","available_bytes":available_bytes}),
            Err(error) => {
                json!({"outcome":"failed","code":error.code().as_str(),"message":trace_sensitive_text(error.message()).to_json()})
            }
        };
        let _ = trace.finish(TraceValue::Data(result_value));
    }
    result
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
    let trace = trace_current(
        Component::Core,
        "linux_volume::validate_linux_volume",
        || {
            vec![
                TraceField::new("config", trace_sensitive_text(&format!("{config:#?}"))),
                TraceField::new("target", TraceValue::path(target)),
                TraceField::new("image_bytes", TraceValue::Data(json!(image_bytes))),
            ]
        },
    );
    let result = (|| {
        let info = inspect_linux_volume(target)?;
        validate_volume_info(config, target, image_bytes, info)
    })();
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(info) => json!({
                "outcome": "succeeded",
                "mount_target": info.mount_target.as_ref().map(|path| TraceValue::path(path).to_json()),
                "filesystem": info.filesystem,
                "uuid": info.uuid,
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

fn validate_volume_info(
    config: &AccountsConfig,
    target: &Path,
    image_bytes: u64,
    info: LinuxVolumeInfo,
) -> Result<LinuxVolumeInfo, OperationError> {
    let trace = trace_current(
        Component::Core,
        "linux_volume::validate_volume_info",
        || {
            vec![
                TraceField::new("config", trace_sensitive_text(&format!("{config:#?}"))),
                TraceField::new("target", TraceValue::path(target)),
                TraceField::new("image_bytes", TraceValue::Data(json!(image_bytes))),
                TraceField::new("volume_info", trace_sensitive_text(&format!("{info:#?}"))),
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
    })();
    if let Some(trace) = trace {
        let result_value = match &result {
            Ok(info) => json!({
                "outcome": "succeeded",
                "mount_target": info.mount_target.as_ref().map(|path| TraceValue::path(path).to_json()),
                "filesystem": info.filesystem,
                "uuid": info.uuid,
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

fn expected_uuid(config: &AccountsConfig, enforce_person_uuid: bool) -> Option<&str> {
    let trace = trace_current(Component::Core, "linux_volume::expected_uuid", || {
        vec![
            TraceField::new("config", trace_sensitive_text(&format!("{config:#?}"))),
            TraceField::new(
                "enforce_person_uuid",
                TraceValue::Data(json!(enforce_person_uuid)),
            ),
        ]
    });
    let result = if let Some(uuid) = config.usb.usb_uuid.as_deref() {
        Some(uuid)
    } else if !enforce_person_uuid {
        None
    } else {
        let mut uuids = config
            .enabled_people()
            .map(|person| person.usb_uuid.as_str())
            .filter(|uuid| !uuid.is_empty());
        let first = uuids.next();
        first.filter(|first| uuids.all(|uuid| uuid == *first))
    };
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(result)));
    }
    result
}

fn is_likely_physical_target(target: &Path) -> bool {
    let trace = trace_current(
        Component::Core,
        "linux_volume::is_likely_physical_target",
        || vec![TraceField::new("target", TraceValue::path(target))],
    );
    let resolved = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    let likely_physical = resolved.starts_with("/media") || resolved.starts_with("/run/media");
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!({"resolved": TraceValue::path(&resolved).to_json(), "likely_physical": likely_physical})));
    }
    likely_physical
}

fn find_mount_info(
    target: &Path,
    run_findmnt: impl FnOnce() -> std::io::Result<Output>,
) -> Option<std::collections::BTreeMap<String, String>> {
    let trace = trace_current(Component::Core, "linux_volume::find_mount_info", || {
        vec![
            TraceField::new("target", TraceValue::path(target)),
            TraceField::new(
                "command",
                TraceValue::Data(json!([
                    "findmnt",
                    "--json",
                    "--output",
                    "TARGET,FSTYPE,UUID",
                    "--target",
                    TraceValue::path(target).to_json(),
                ])),
            ),
            TraceField::new(
                "external_internal_execution",
                TraceValue::Omitted(TraceOmission::ExternalComponent),
            ),
        ]
    });
    let output = run_findmnt();
    let result = match &output {
        Ok(output) if output.status.success() => parse_findmnt_output(&output.stdout),
        Ok(_) | Err(_) => None,
    };
    if let Some(trace) = trace {
        use std::os::unix::process::ExitStatusExt;

        let process_result = match &output {
            Ok(output) => json!({
                "outcome": "process_exited",
                "success": output.status.success(),
                "code": output.status.code(),
                "signal": output.status.signal(),
                "stdout": trace_findmnt_payload(&output.stdout).to_json(),
                "stderr": trace_findmnt_payload(&output.stderr).to_json(),
                "external_internal_execution": TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
            }),
            Err(error) => json!({
                "outcome": "spawn_failed",
                "error_kind": format!("{:?}", error.kind()),
                "error": trace_sensitive_text(&error.to_string()).to_json(),
                "external_internal_execution": TraceValue::Omitted(TraceOmission::ExternalComponent).to_json(),
            }),
        };
        let _ = trace.finish(TraceValue::Data(json!({
            "process_result": process_result,
            "mount_info": result,
        })));
    }
    result
}

fn parse_findmnt_output(bytes: &[u8]) -> Option<std::collections::BTreeMap<String, String>> {
    let trace = trace_current(
        Component::Core,
        "linux_volume::parse_findmnt_output",
        || {
            vec![TraceField::new(
                "findmnt_json",
                trace_findmnt_payload(bytes),
            )]
        },
    );
    let result = (|| {
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
    })();
    if let Some(trace) = trace {
        let _ = trace.finish(trace_json_value(&json!(result)));
    }
    result
}

fn trace_findmnt_payload(bytes: &[u8]) -> TraceValue {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(payload) => trace_json_value(&payload),
        Err(_) => std::str::from_utf8(bytes).map_or_else(
            |_| TraceValue::Omitted(TraceOmission::UnsupportedType),
            trace_sensitive_text,
        ),
    }
}

fn usb_error(message: impl Into<String>) -> OperationError {
    let message = message.into();
    let trace = trace_current(Component::Core, "linux_volume::usb_error", || {
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
    use super::{
        LinuxVolumeInfo, calculate_available_bytes, expected_uuid, find_mount_info,
        inspect_linux_volume, is_likely_physical_target, parse_findmnt_output,
        trace_statvfs_result, validate_volume_info,
    };
    use crate::config::{AccountsConfig, PersonConfig, UsbConfig};
    use crate::logging::{Component, Level, Session};
    use rustix::fs::{StatVfs, StatVfsMountFlags};
    use serde_json::{Value, json};
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::{ExitStatus, Output};

    fn process_output(code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    fn statvfs_fixture(free_blocks: u64, fragment_size: u64, block_size: u64) -> StatVfs {
        StatVfs {
            f_bsize: block_size,
            f_frsize: fragment_size,
            f_blocks: 100,
            f_bfree: free_blocks,
            f_bavail: free_blocks.saturating_sub(1),
            f_files: 50,
            f_ffree: 25,
            f_favail: 24,
            f_fsid: 42,
            f_flag: StatVfsMountFlags::RDONLY | StatVfsMountFlags::NOEXEC,
            f_namemax: 255,
        }
    }

    fn assert_correlated_spans(records: &[Value], function: &str, expected_count: usize) {
        let entries: Vec<_> = records
            .iter()
            .filter(|record| record["type"] == "function_enter" && record["function"] == function)
            .collect();
        let exits: Vec<_> = records
            .iter()
            .filter(|record| record["type"] == "function_exit" && record["function"] == function)
            .collect();
        assert_eq!(entries.len(), expected_count, "ingressi {function}");
        assert_eq!(exits.len(), expected_count, "uscite {function}");
        for entry in entries {
            let call_id = entry["call_id"].as_u64().expect("call_id numerico");
            let matching: Vec<_> = exits
                .iter()
                .filter(|exit| exit["call_id"] == call_id)
                .collect();
            assert_eq!(matching.len(), 1, "uscita correlata {function}#{call_id}");
            assert_eq!(matching[0]["outcome"], "returned");
            assert!(
                matching[0]["duration_ms"].is_u64(),
                "durata assente {function}#{call_id}"
            );
        }
    }

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
    fn synthetic_statvfs_fields_and_available_space_calculation_are_traced() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-linux-statvfs-trace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, log_path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione Trace sintetica");
        let fragment_stats = statvfs_fixture(5, 1024, 4096);
        let fallback_stats = statvfs_fixture(3, 0, 4096);
        let fragment_result: rustix::io::Result<StatVfs> = Ok(fragment_stats);
        let projected = trace_statvfs_result(&fragment_result);
        assert_eq!(projected["f_bfree"], 5);
        assert_eq!(projected["f_frsize"], 1024);
        assert_eq!(projected["f_bsize"], 4096);
        assert_eq!(projected["f_blocks"], 100);
        assert_eq!(projected["f_bavail"], 4);
        assert_eq!(projected["f_files"], 50);
        assert_eq!(projected["f_ffree"], 25);
        assert_eq!(projected["f_favail"], 24);
        assert_eq!(projected["f_fsid"], 42);
        assert_eq!(projected["f_namemax"], 255);
        assert_eq!(
            projected["f_flag_bits"],
            (StatVfsMountFlags::RDONLY | StatVfsMountFlags::NOEXEC).bits()
        );

        let (available_frag, available_fallback, inspected) = session.scoped(|| {
            (
                calculate_available_bytes(fragment_result.as_ref().unwrap())
                    .expect("fragmentsize sintetico"),
                calculate_available_bytes(&fallback_stats).expect("fallback block size sintetico"),
                inspect_linux_volume(&directory).expect("statvfs directory sintetica"),
            )
        });
        drop(session);

        assert_eq!(available_frag, 5 * 1024);
        assert_eq!(available_fallback, 3 * 4096);
        assert!(inspected.available_bytes > 0);
        let contents = fs::read_to_string(&log_path).expect("log statvfs sintetico");
        let records: Vec<Value> = contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("record JSON"))
            .collect();
        assert_correlated_spans(&records, "linux_volume::calculate_available_bytes", 3);
        assert!(contents.contains("\"name\":\"f_frsize\""));
        assert!(contents.contains("\"value\":1024"));
        assert!(contents.contains("\"available_bytes\":5120"));
        assert!(contents.contains("\"value\":0"));
        assert!(contents.contains("\"available_bytes\":12288"));
        let inspect_exit = records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "linux_volume::inspect_linux_volume"
            })
            .expect("uscita inspect_linux_volume");
        assert!(inspect_exit["result"]["value"]["statvfs_result"]["f_bfree"].is_number());
        assert!(inspect_exit["result"]["value"]["statvfs_result"]["f_frsize"].is_number());
        assert!(inspect_exit["result"]["value"]["statvfs_result"]["f_bsize"].is_number());
        assert!(inspect_exit["result"]["value"]["statvfs_result"]["f_namemax"].is_number());
        assert!(inspect_exit["result"]["value"]["statvfs_result"]["f_flag_bits"].is_number());
        fs::remove_file(log_path).expect("rimozione log sintetico");
        fs::remove_dir_all(directory).expect("rimozione directory sintetica");
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

    #[test]
    // Fixture sintetiche e directory locale: non è una prova con USB fisica.
    fn synthetic_logging_separates_trace_details_from_events() {
        for (suffix, level) in [("events", Level::Events), ("trace", Level::Trace)] {
            let directory = std::env::temp_dir().join(format!(
                "sanikey-linux-volume-{suffix}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let (session, path) =
                Session::create_in_directory_with_level(&directory, Component::Core, level)
                    .expect("sessione sintetica");
            let configured = config(Some("FIXTURE-UUID"), &[("FIXTURE-UUID", true)]);
            let target = PathBuf::from("/media/synthetic");
            let payload = br#"{"filesystems":[{"target":"/media/synthetic","fstype":"exfat","uuid":"FIXTURE-UUID","api_key":"synthetic-findmnt-secret","label":"private-mount-label"}]}"#;
            session.scoped(|| {
                assert_eq!(expected_uuid(&configured, true), Some("FIXTURE-UUID"));
                assert!(is_likely_physical_target(&target));
                assert!(parse_findmnt_output(payload).is_some());
                assert!(
                    find_mount_info(std::env::temp_dir().as_path(), || {
                        Ok(process_output(0, payload, b"synthetic findmnt warning"))
                    })
                    .is_some()
                );
                validate_volume_info(
                    &configured,
                    &target,
                    1024,
                    volume(Some("exfat"), Some("FIXTURE-UUID"), 2 * 1024 * 1024),
                )
                .expect("fixture volume valida");
                assert!(
                    validate_volume_info(
                        &configured,
                        &target,
                        1024,
                        volume(Some("exfat"), Some("OTHER-UUID"), 2 * 1024 * 1024),
                    )
                    .is_err()
                );
            });
            drop(session);

            let contents = fs::read_to_string(&path).expect("log sintetico");
            if level == Level::Trace {
                assert!(contents.contains("linux_volume::expected_uuid"));
                assert!(contents.contains("linux_volume::is_likely_physical_target"));
                assert!(contents.contains("linux_volume::parse_findmnt_output"));
                assert!(contents.contains("linux_volume::find_mount_info"));
                assert!(contents.contains("linux_volume::validate_volume_info"));
                assert!(contents.contains("linux_volume::usb_error"));
                assert!(contents.contains("external_component"));
                assert!(contents.contains("config_version"));
                assert!(contents.contains("Persona sintetica"));
                assert!(contents.contains("FIXTURE-UUID"));
                assert!(contents.contains("private-mount-label"));
                assert!(contents.contains("synthetic findmnt warning"));
                assert!(contents.contains("secret_omitted"));
                assert!(!contents.contains("synthetic-findmnt-secret"));
            } else {
                assert!(!contents.contains("function_enter"));
                assert!(!contents.contains("FIXTURE-UUID"));
                assert!(!contents.contains("Persona sintetica"));
                assert!(!contents.contains("synthetic findmnt warning"));
                assert!(!contents.contains("synthetic-findmnt-secret"));
            }
            fs::remove_file(path).expect("rimozione log sintetico");
            fs::remove_dir_all(directory).expect("rimozione directory sintetica");
        }
    }

    #[test]
    fn findmnt_trace_records_exit_streams_and_spawn_errors() {
        let directory = std::env::temp_dir().join(format!(
            "sanikey-linux-findmnt-boundary-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let (session, log_path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione Trace sintetica");
        let target = PathBuf::from("/media/synthetic");
        let nonzero_stdout = br#"{"filesystems":[]}"#;
        let nonzero_stderr = b"synthetic findmnt exit failure";

        let failed_exit = session.scoped(|| {
            find_mount_info(&target, || {
                Ok(process_output(7, nonzero_stdout, nonzero_stderr))
            })
        });
        let failed_spawn = session.scoped(|| {
            find_mount_info(&target, || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "synthetic findmnt executable missing",
                ))
            })
        });
        drop(session);

        assert!(failed_exit.is_none());
        assert!(failed_spawn.is_none());
        let contents = fs::read_to_string(&log_path).expect("log findmnt sintetico");
        assert!(contents.contains("synthetic findmnt exit failure"));
        assert!(contents.contains("synthetic findmnt executable missing"));
        assert!(contents.contains("\"code\":7"));
        assert!(contents.contains("\"outcome\":\"spawn_failed\""));
        assert!(contents.contains("\"error_kind\":\"NotFound\""));
        assert!(contents.contains("external_internal_execution"));
        assert!(contents.contains("external_component"));
        let records: Vec<Value> = contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("record JSON"))
            .collect();
        assert_correlated_spans(&records, "linux_volume::find_mount_info", 2);
        let expected_command = json!([
            "findmnt",
            "--json",
            "--output",
            "TARGET,FSTYPE,UUID",
            "--target",
            {
                "kind": "data",
                "value": {
                    "encoding": "utf8",
                    "value": target.to_str().expect("target sintetico UTF-8"),
                }
            }
        ]);
        for entry in records.iter().filter(|record| {
            record["type"] == "function_enter"
                && record["function"] == "linux_volume::find_mount_info"
        }) {
            let command = entry["arguments"]
                .as_array()
                .expect("argomenti")
                .iter()
                .find(|argument| argument["name"] == "command")
                .expect("argv findmnt");
            assert_eq!(command["content"]["value"], expected_command);
        }
        fs::remove_file(log_path).expect("rimozione log sintetico");
        fs::remove_dir_all(directory).expect("rimozione directory sintetica");
    }
}
