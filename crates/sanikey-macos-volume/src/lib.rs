//! Lettura in sola lettura dei metadati di volume su macOS.

#![deny(unsafe_code)]

#[cfg(any(target_os = "macos", test))]
use std::io;
#[cfg(target_os = "macos")]
use std::path::Path;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(any(target_os = "macos", test))]
use std::time::Instant;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod ffi;

/// Identità nativa VolumeUUID di un volume macOS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacVolumeUuid(String);

/// Espone il VolumeUUID nativo senza convertirlo in un identificatore diverso.
impl MacVolumeUuid {
    /// Restituisce l'identità nativa del volume senza convertirla in UUID Linux.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Metadati in sola lettura del volume macOS che contiene il target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacVolumeInfo {
    /// Punto di mount restituito dal kernel macOS.
    pub mount_point: std::path::PathBuf,
    /// Identità VolumeUUID restituita da `diskutil`.
    pub identity: Option<MacVolumeUuid>,
    /// Tipo filesystem restituito dal kernel macOS.
    pub filesystem: String,
    /// Byte disponibili per il chiamante sul filesystem.
    pub available_bytes: u64,
}

/// Osservazione sintetica di una chiamata nativa o al tool `diskutil`.
///
/// L'output di `diskutil` contiene metadati privati del volume e i byte già
/// catturati dal processo; non viene eseguita una lettura aggiuntiva.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MacVolumeObservation {
    /// Esito di `statfs` e proiezione dei campi leggibili del kernel.
    Statfs {
        target: PathBuf,
        /// Microsecondi misurati attorno alla chiamata `statfs`.
        duration_micros: u64,
        success: bool,
        mount_point: Option<PathBuf>,
        filesystem: Option<String>,
        available_bytes: Option<u64>,
        error_code: Option<i32>,
    },
    /// Esito di `diskutil info -plist` e byte stdout/stderr del processo.
    Diskutil {
        mount_point: PathBuf,
        /// Microsecondi misurati attorno a spawn, esecuzione e cattura output.
        duration_micros: u64,
        success: bool,
        exit_code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        error_code: Option<i32>,
    },
    /// Parsing dei byte restituiti da `diskutil info -plist`.
    PlistParse {
        xml: Vec<u8>,
        duration_micros: u64,
        success: bool,
        volume_uuid: Option<String>,
        error_kind: Option<std::io::ErrorKind>,
        error_message: Option<String>,
    },
}

#[cfg(any(target_os = "macos", test))]
fn measure_call<T>(call: impl FnOnce() -> T) -> (T, u64) {
    let started = Instant::now();
    let result = call();
    let duration_micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    (result, duration_micros)
}

/// Legge i metadati di un volume già montato senza scriverci.
///
/// Il target deve essere una directory esistente. Un VolumeUUID assente viene
/// riportato come non disponibile; volume smontato ed errori sono espliciti.
#[cfg(target_os = "macos")]
pub fn inspect(target: &Path) -> io::Result<MacVolumeInfo> {
    inspect_with_observer(target, |_| {})
}

/// Variante osservabile di [`inspect`] per il logger dell'applicazione.
///
/// Il callback riceve l'esito dei confini `statfs` e `diskutil`. La API
/// ordinaria `inspect` resta invariata e usa un callback vuoto.
#[cfg(target_os = "macos")]
pub fn inspect_with_observer(
    target: &Path,
    mut observer: impl FnMut(MacVolumeObservation),
) -> io::Result<MacVolumeInfo> {
    if !target.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "il target macOS deve essere una directory esistente",
        ));
    }
    let absolute_target = target.canonicalize()?;
    let (mount_point, filesystem, available_bytes) =
        ffi::inspect_statfs(&absolute_target, &mut observer)?;

    let (output_result, duration_micros) = measure_call(|| {
        Command::new("/usr/sbin/diskutil")
            .args(["info", "-plist"])
            .arg(&mount_point)
            .output()
    });
    let output = match output_result {
        Ok(output) => {
            observer(MacVolumeObservation::Diskutil {
                mount_point: mount_point.clone(),
                duration_micros,
                success: output.status.success(),
                exit_code: output.status.code(),
                stdout: output.stdout.clone(),
                stderr: output.stderr.clone(),
                error_code: None,
            });
            output
        }
        Err(error) => {
            observer(MacVolumeObservation::Diskutil {
                mount_point: mount_point.clone(),
                duration_micros,
                success: false,
                exit_code: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
                error_code: error.raw_os_error(),
            });
            return Err(error);
        }
    };
    if !output.status.success() {
        return Err(io::Error::other(
            "diskutil non ha restituito i metadati del volume",
        ));
    }
    let volume_uuid = parse_volume_uuid_plist_with_observer(&output.stdout, &mut observer)?;
    Ok(MacVolumeInfo {
        mount_point,
        identity: volume_uuid.map(MacVolumeUuid),
        filesystem,
        available_bytes,
    })
}

#[cfg(any(target_os = "macos", test))]
fn parse_volume_uuid_plist(xml: &[u8]) -> io::Result<Option<String>> {
    parse_volume_uuid_plist_with_observer(xml, |_| {})
}

#[cfg(any(target_os = "macos", test))]
fn parse_volume_uuid_plist_with_observer(
    xml: &[u8],
    mut observer: impl FnMut(MacVolumeObservation),
) -> io::Result<Option<String>> {
    let started = Instant::now();
    let result = parse_volume_uuid_plist_xml(xml);
    observer(MacVolumeObservation::PlistParse {
        xml: xml.to_vec(),
        duration_micros: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        success: result.is_ok(),
        volume_uuid: result.as_ref().ok().and_then(Clone::clone),
        error_kind: result.as_ref().err().map(io::Error::kind),
        error_message: result.as_ref().err().map(ToString::to_string),
    });
    result
}

#[cfg(any(target_os = "macos", test))]
fn parse_volume_uuid_plist_xml(xml: &[u8]) -> io::Result<Option<String>> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut reading_key = false;
    let mut reading_uuid = false;
    let mut next_key_is_uuid = false;
    let mut root_seen = false;
    let mut depth = 0_u32;
    let mut volume_uuid = None;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => {
                let name = element.local_name();
                root_seen |= name.as_ref() == b"plist";
                depth = depth.saturating_add(1);
                if name.as_ref() == b"key" {
                    reading_key = true;
                } else if name.as_ref() == b"string" {
                    reading_uuid = next_key_is_uuid;
                    next_key_is_uuid = false;
                }
            }
            Ok(Event::Empty(element)) => {
                root_seen |= element.local_name().as_ref() == b"plist";
            }
            Ok(Event::Text(text)) if reading_key => {
                let value = text
                    .xml_content(quick_xml::XmlVersion::Explicit1_0)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                next_key_is_uuid = value == "VolumeUUID";
                reading_key = false;
            }
            Ok(Event::Text(text)) if reading_uuid => {
                let value = text
                    .xml_content(quick_xml::XmlVersion::Explicit1_0)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if value.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "VolumeUUID macOS vuoto",
                    ));
                }
                volume_uuid = Some(value.into_owned());
                reading_uuid = false;
            }
            Ok(Event::End(element)) => {
                if element.local_name().as_ref() == b"string" {
                    reading_uuid = false;
                }
                depth = depth.checked_sub(1).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "chiusura plist non valida")
                })?;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
        }
        buffer.clear();
    }
    if !root_seen || depth != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "property list diskutil incompleta o non valida",
        ));
    }
    Ok(volume_uuid)
}

#[cfg(test)]
mod tests {
    use super::{measure_call, parse_volume_uuid_plist, parse_volume_uuid_plist_with_observer};
    use crate::MacVolumeObservation;
    use std::io::ErrorKind;

    #[test]
    fn extracts_the_volume_uuid_from_diskutil_property_list() {
        let plist = br#"<?xml version="1.0" encoding="UTF-8"?>
            <plist version="1.0"><dict>
            <key>FilesystemName</key><string>ExFAT</string>
            <key>VolumeUUID</key><string>ABCD-1234</string>
            </dict></plist>"#;
        assert_eq!(
            parse_volume_uuid_plist(plist).unwrap().as_deref(),
            Some("ABCD-1234")
        );
    }

    #[test]
    fn represents_missing_volume_uuid_and_rejects_malformed_property_lists() {
        assert_eq!(
            parse_volume_uuid_plist(b"<plist><dict/></plist>").unwrap(),
            None
        );
        assert_eq!(
            parse_volume_uuid_plist(b"<plist><").unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            parse_volume_uuid_plist(b"not xml").unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            parse_volume_uuid_plist(b"<plist><dict><key>VolumeUUID</key><string>ID</string>")
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
    }

    #[test]
    fn observed_parser_reports_synthetic_input_result_and_duration() {
        let plist =
            br#"<plist><dict><key>VolumeUUID</key><string>SYNTHETIC-UUID</string></dict></plist>"#;
        let mut observations = Vec::new();
        let parsed = parse_volume_uuid_plist_with_observer(plist, |observation| {
            observations.push(observation);
        })
        .expect("plist sintetica valida");

        assert_eq!(parsed.as_deref(), Some("SYNTHETIC-UUID"));
        assert!(matches!(
            observations.as_slice(),
            [MacVolumeObservation::PlistParse {
                xml,
                duration_micros: _,
                success: true,
                volume_uuid: Some(uuid),
                error_kind: None,
                error_message: None,
            }] if xml == plist && uuid == "SYNTHETIC-UUID"
        ));

        let mut failed = Vec::new();
        assert!(
            parse_volume_uuid_plist_with_observer(b"<plist><", |observation| {
                failed.push(observation);
            })
            .is_err()
        );
        assert!(matches!(
            failed.as_slice(),
            [MacVolumeObservation::PlistParse {
                xml,
                success: false,
                volume_uuid: None,
                error_kind: Some(ErrorKind::InvalidData),
                error_message: Some(_),
                ..
            }] if xml == b"<plist><"
        ));
    }

    #[test]
    fn measured_call_includes_synthetic_call_duration() {
        let (value, duration_micros) = measure_call(|| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            23
        });

        assert_eq!(value, 23);
        assert!(duration_micros >= 1_000);
    }
}
