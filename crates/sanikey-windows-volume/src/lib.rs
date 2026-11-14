//! Confine sicuro per la lettura Windows dei metadati volume via Win32.

#![deny(unsafe_code)]

#[cfg(any(windows, test))]
use std::io;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(any(windows, test))]
use std::time::Instant;

#[cfg(windows)]
#[allow(unsafe_code)]
mod ffi;

/// Identità Volume GUID restituita dal volume manager di Windows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsVolumeGuidPath(String);

/// Espone il Volume GUID path nella forma nativa di Windows.
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

/// Osservazione sintetica di una singola chiamata Win32 effettuata da `inspect`.
///
/// I dati rappresentano input e output restituiti dall'API; non includono
/// contenuti del filesystem né handle nativi.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowsVolumeObservation {
    /// Ricerca del mount path che contiene il target.
    VolumePath {
        target: PathBuf,
        /// Microsecondi misurati attorno alla chiamata Win32.
        duration_micros: u64,
        success: bool,
        mount_path: Option<PathBuf>,
        error_code: Option<i32>,
    },
    /// Lettura del Volume GUID associato al mount path.
    VolumeGuid {
        mount_path: PathBuf,
        /// Microsecondi misurati attorno alla chiamata Win32.
        duration_micros: u64,
        success: bool,
        volume_guid_path: Option<String>,
        error_code: Option<i32>,
    },
    /// Lettura del nome filesystem.
    Filesystem {
        mount_path: PathBuf,
        /// Microsecondi misurati attorno alla chiamata Win32.
        duration_micros: u64,
        success: bool,
        filesystem: Option<String>,
        error_code: Option<i32>,
    },
    /// Lettura dello spazio disponibile per il chiamante.
    AvailableSpace {
        target: PathBuf,
        /// Microsecondi misurati attorno alla chiamata Win32.
        duration_micros: u64,
        success: bool,
        available_bytes: Option<u64>,
        error_code: Option<i32>,
    },
}

#[cfg(any(windows, test))]
fn measure_call<T>(call: impl FnOnce() -> T) -> (T, u64) {
    let started = Instant::now();
    let result = call();
    let duration_micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    (result, duration_micros)
}

/// Provides access to the inspected Windows volume properties.
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
    inspect_with_observer(target, |_| {})
}

/// Variante osservabile di [`inspect`] per il logger dell'applicazione.
///
/// Il callback riceve una voce per ogni chiamata Win32 raggiunta. `inspect`
/// rimane la API ordinaria e usa un callback vuoto.
#[cfg(windows)]
pub fn inspect_with_observer(
    target: &Path,
    observer: impl FnMut(WindowsVolumeObservation),
) -> io::Result<WindowsVolumeInfo> {
    if !target.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "il target Windows deve essere una directory esistente",
        ));
    }
    let absolute_target = target.canonicalize()?;
    ffi::inspect(&absolute_target, observer)
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
    use super::{measure_call, wide_to_string};
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

    #[test]
    fn measured_call_includes_synthetic_call_duration() {
        let (value, duration_micros) = measure_call(|| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            17
        });

        assert_eq!(value, 17);
        assert!(duration_micros >= 1_000);
    }
}
