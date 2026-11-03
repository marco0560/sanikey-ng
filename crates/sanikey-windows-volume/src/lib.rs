//! Confine sicuro per la lettura Windows dei metadati volume via Win32.

#![deny(unsafe_code)]

#[cfg(any(windows, test))]
use std::io;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(windows)]
#[allow(unsafe_code)]
mod ffi;

/// Identità Volume GUID restituita dal volume manager di Windows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsVolumeGuidPath(String);

impl WindowsVolumeGuidPath {
    /// Restituisce il Volume GUID path nella forma nativa restituita da Windows.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Metadati in sola lettura del volume Windows contenente il target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsVolumeInfo {
    /// Percorso di mount del volume, normalmente una lettera con barra finale.
    pub mount_path: PathBuf,
    /// Identità nativa; non è un UUID filesystem né un seriale di formattazione.
    pub identity: WindowsVolumeGuidPath,
    /// Nome filesystem restituito da `GetVolumeInformationW`.
    pub filesystem: String,
    /// Byte disponibili per il chiamante restituiti da `GetDiskFreeSpaceExW`.
    pub available_bytes: u64,
}

impl WindowsVolumeInfo {
    #[cfg(windows)]
    pub(crate) fn from_api_parts(
        mount_path: PathBuf,
        volume_guid_path: String,
        filesystem: String,
        available_bytes: u64,
    ) -> Self {
        Self {
            mount_path,
            identity: WindowsVolumeGuidPath(volume_guid_path),
            filesystem,
            available_bytes,
        }
    }
}

/// Legge metadati di volume senza creare directory o scrivere sul filesystem.
///
/// Il target deve già esistere. Su piattaforme diverse da Windows l'adattatore
/// nativo non è disponibile.
#[cfg(windows)]
pub fn inspect(target: &Path) -> io::Result<WindowsVolumeInfo> {
    if !target.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "il target Windows deve essere una directory esistente",
        ));
    }
    let absolute_target = target.canonicalize()?;
    ffi::inspect(&absolute_target)
}

#[cfg(any(windows, test))]
fn terminated_wide(buffer: &[u16]) -> io::Result<&[u16]> {
    let end = buffer.iter().position(|unit| *unit == 0).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "risposta Win32 senza terminatore UTF-16",
        )
    })?;
    Ok(&buffer[..end])
}

#[cfg(any(windows, test))]
fn wide_to_string(buffer: &[u16]) -> io::Result<String> {
    String::from_utf16(terminated_wide(buffer)?).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("UTF-16 Win32 non valido: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::wide_to_string;
    use std::io::ErrorKind;

    #[test]
    fn decodes_null_terminated_utf16_and_rejects_malformed_output() {
        assert_eq!(wide_to_string(&[b'X' as u16, 0]).unwrap(), "X");
        assert_eq!(wide_to_string(&[0x03a9, 0]).unwrap(), "Ω");
        assert_eq!(
            wide_to_string(&[b'X' as u16]).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            wide_to_string(&[0xd800, 0]).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
    }
}
