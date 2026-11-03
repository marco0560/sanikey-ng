//! Lettura in sola lettura dei metadati di volume su macOS.

#![deny(unsafe_code)]

#[cfg(any(target_os = "macos", test))]
use std::io;
#[cfg(target_os = "macos")]
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod ffi;

/// Identità nativa VolumeUUID di un volume macOS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacVolumeUuid(String);

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

/// Legge i metadati di un volume già montato senza scriverci.
///
/// Il target deve essere una directory esistente. Un VolumeUUID assente viene
/// riportato come non disponibile; volume smontato ed errori sono espliciti.
#[cfg(target_os = "macos")]
pub fn inspect(target: &Path) -> io::Result<MacVolumeInfo> {
    if !target.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "il target macOS deve essere una directory esistente",
        ));
    }
    let absolute_target = target.canonicalize()?;
    let (mount_point, filesystem, available_bytes) = ffi::inspect_statfs(&absolute_target)?;

    let output = Command::new("/usr/sbin/diskutil")
        .args(["info", "-plist"])
        .arg(&mount_point)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "diskutil non ha restituito i metadati del volume",
        ));
    }
    let volume_uuid = parse_volume_uuid_plist(&output.stdout)?;
    Ok(MacVolumeInfo {
        mount_point,
        identity: volume_uuid.map(MacVolumeUuid),
        filesystem,
        available_bytes,
    })
}

#[cfg(any(target_os = "macos", test))]
fn parse_volume_uuid_plist(xml: &[u8]) -> io::Result<Option<String>> {
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
    use super::parse_volume_uuid_plist;
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
}
