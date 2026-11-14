//! Confine FFI macOS per la lettura di `statfs`.

use crate::MacVolumeObservation;
use std::ffi::{CStr, CString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub(super) fn inspect_statfs(
    target: &Path,
    observer: &mut impl FnMut(MacVolumeObservation),
) -> io::Result<(PathBuf, String, u64)> {
    let target_c = CString::new(target.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "il percorso contiene byte NUL")
    })?;
    let mut statistics = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: target_c is NUL-terminated and statistics points to a zeroed
    // writable statfs structure. The result is read only after a zero status.
    let ((status, api_error), duration_micros) = crate::measure_call(|| {
        // SAFETY: target_c is NUL-terminated and statistics points to a zeroed
        // writable statfs structure.
        let status = unsafe { libc::statfs(target_c.as_ptr(), statistics.as_mut_ptr()) };
        let error = (status != 0).then(io::Error::last_os_error);
        (status, error)
    });
    if status != 0 {
        let error = api_error.expect("statfs error captured at call boundary");
        observer(MacVolumeObservation::Statfs {
            target: target.to_path_buf(),
            duration_micros,
            success: false,
            mount_point: None,
            filesystem: None,
            available_bytes: None,
            error_code: error.raw_os_error(),
        });
        return Err(error);
    }
    // SAFETY: successful statfs initialized the output structure.
    let statistics = unsafe { statistics.assume_init() };
    let result = (|| {
        let mount_point = PathBuf::from(std::ffi::OsStr::from_bytes(c_string_bytes(
            &statistics.f_mntonname,
        )?));
        let filesystem = std::str::from_utf8(c_string_bytes(&statistics.f_fstypename)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
            .to_owned();
        let available_bytes = statistics
            .f_bavail
            .checked_mul(u64::from(statistics.f_bsize))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "spazio libero fuori intervallo")
            })?;
        Ok((mount_point, filesystem, available_bytes))
    })();
    match result {
        Ok((mount_point, filesystem, available_bytes)) => {
            observer(MacVolumeObservation::Statfs {
                target: target.to_path_buf(),
                duration_micros,
                success: true,
                mount_point: Some(mount_point.clone()),
                filesystem: Some(filesystem.clone()),
                available_bytes: Some(available_bytes),
                error_code: None,
            });
            Ok((mount_point, filesystem, available_bytes))
        }
        Err(error) => {
            observer(MacVolumeObservation::Statfs {
                target: target.to_path_buf(),
                duration_micros,
                success: true,
                mount_point: None,
                filesystem: None,
                available_bytes: None,
                error_code: None,
            });
            Err(error)
        }
    }
}

fn c_string_bytes(buffer: &[libc::c_char]) -> io::Result<&[u8]> {
    CStr::from_bytes_until_nul(
        // SAFETY: c_char is a one-byte C character and this is a fixed-size
        // libc output array; the cast preserves its exact byte length.
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), buffer.len()) },
    )
    .map(CStr::to_bytes)
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
