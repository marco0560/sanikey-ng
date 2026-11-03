use std::ffi::{CStr, CString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub(super) fn inspect_statfs(target: &Path) -> io::Result<(PathBuf, String, u64)> {
    let target_c = CString::new(target.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "il percorso contiene byte NUL")
    })?;
    let mut statistics = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: target_c is NUL-terminated and statistics points to a zeroed
    // writable statfs structure. The result is read only after a zero status.
    let status = unsafe { libc::statfs(target_c.as_ptr(), statistics.as_mut_ptr()) };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful statfs initialized the output structure.
    let statistics = unsafe { statistics.assume_init() };
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
