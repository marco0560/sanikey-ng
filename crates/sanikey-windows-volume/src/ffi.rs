use crate::{WindowsVolumeInfo, wide_to_string};
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNameW,
};

const PATH_BUFFER_UNITS: usize = 32_768;
const VOLUME_GUID_BUFFER_UNITS: usize = 128;
const FILESYSTEM_BUFFER_UNITS: usize = 256;

pub(super) fn inspect(target: &Path) -> io::Result<WindowsVolumeInfo> {
    let target_wide = encode_wide(OsStr::new(target));
    let mut mount_buffer = vec![0_u16; PATH_BUFFER_UNITS];
    let success = unsafe {
        // SAFETY: both pointers address valid UTF-16 buffers; the output length
        // matches the allocated capacity and the input is NUL-terminated.
        GetVolumePathNameW(
            target_wide.as_ptr(),
            mount_buffer.as_mut_ptr(),
            mount_buffer.len() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    let mount_units = crate::terminated_wide(&mount_buffer)?;
    let mount_path = PathBuf::from(std::ffi::OsString::from_wide(mount_units));
    let mount_wide = encode_wide(mount_path.as_os_str());

    let mut guid_buffer = vec![0_u16; VOLUME_GUID_BUFFER_UNITS];
    let success = unsafe {
        // SAFETY: mount_wide is NUL-terminated and guid_buffer is writable for
        // the explicit number of UTF-16 code units passed to Win32.
        GetVolumeNameForVolumeMountPointW(
            mount_wide.as_ptr(),
            guid_buffer.as_mut_ptr(),
            guid_buffer.len() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    let volume_guid_path = wide_to_string(&guid_buffer)?;

    let mut filesystem_buffer = vec![0_u16; FILESYSTEM_BUFFER_UNITS];
    let success = unsafe {
        // SAFETY: mount_wide is NUL-terminated; optional outputs are null as
        // documented, and the filesystem buffer length matches its allocation.
        GetVolumeInformationW(
            mount_wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            filesystem_buffer.as_mut_ptr(),
            filesystem_buffer.len() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    let filesystem = wide_to_string(&filesystem_buffer)?;

    let mut available_bytes = 0_u64;
    let success = unsafe {
        // SAFETY: target_wide is NUL-terminated and available_bytes is a valid
        // writable u64 output. The total-size outputs are optional and null.
        GetDiskFreeSpaceExW(
            target_wide.as_ptr(),
            &mut available_bytes,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(WindowsVolumeInfo::from_api_parts(
        mount_path,
        volume_guid_path,
        filesystem,
        available_bytes,
    ))
}

fn encode_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}
