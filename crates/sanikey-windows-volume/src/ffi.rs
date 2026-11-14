//! Confine Win32 per mount, Volume GUID, filesystem e spazio disponibile.

use crate::{WindowsVolumeInfo, WindowsVolumeObservation, wide_to_string};
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

pub(super) fn inspect(
    target: &Path,
    mut observer: impl FnMut(WindowsVolumeObservation),
) -> io::Result<WindowsVolumeInfo> {
    let target_wide = encode_wide(OsStr::new(target));
    let mut mount_buffer = vec![0_u16; PATH_BUFFER_UNITS];
    let ((success, api_error), duration_micros) = crate::measure_call(|| {
        // SAFETY: both pointers address valid UTF-16 buffers; the output length
        // matches the allocated capacity and the input is NUL-terminated.
        let success = unsafe {
            GetVolumePathNameW(
                target_wide.as_ptr(),
                mount_buffer.as_mut_ptr(),
                mount_buffer.len() as u32,
            )
        };
        let error = (success == 0).then(io::Error::last_os_error);
        (success, error)
    });
    if success == 0 {
        let error = api_error.expect("Win32 error captured at call boundary");
        observer(WindowsVolumeObservation::VolumePath {
            target: target.to_path_buf(),
            duration_micros,
            success: false,
            mount_path: None,
            error_code: error.raw_os_error(),
        });
        return Err(error);
    }
    let mount_units = match crate::terminated_wide(&mount_buffer) {
        Ok(units) => units,
        Err(error) => {
            observer(WindowsVolumeObservation::VolumePath {
                target: target.to_path_buf(),
                duration_micros,
                success: true,
                mount_path: None,
                error_code: None,
            });
            return Err(error);
        }
    };
    let mount_path = PathBuf::from(std::ffi::OsString::from_wide(mount_units));
    observer(WindowsVolumeObservation::VolumePath {
        target: target.to_path_buf(),
        duration_micros,
        success: true,
        mount_path: Some(mount_path.clone()),
        error_code: None,
    });
    let mount_wide = encode_wide(mount_path.as_os_str());

    let mut guid_buffer = vec![0_u16; VOLUME_GUID_BUFFER_UNITS];
    let ((success, api_error), duration_micros) = crate::measure_call(|| {
        // SAFETY: mount_wide is NUL-terminated and guid_buffer is writable for
        // the explicit number of UTF-16 code units passed to Win32.
        let success = unsafe {
            GetVolumeNameForVolumeMountPointW(
                mount_wide.as_ptr(),
                guid_buffer.as_mut_ptr(),
                guid_buffer.len() as u32,
            )
        };
        let error = (success == 0).then(io::Error::last_os_error);
        (success, error)
    });
    if success == 0 {
        let error = api_error.expect("Win32 error captured at call boundary");
        observer(WindowsVolumeObservation::VolumeGuid {
            mount_path: mount_path.clone(),
            duration_micros,
            success: false,
            volume_guid_path: None,
            error_code: error.raw_os_error(),
        });
        return Err(error);
    }
    let volume_guid_path = match wide_to_string(&guid_buffer) {
        Ok(value) => value,
        Err(error) => {
            observer(WindowsVolumeObservation::VolumeGuid {
                mount_path: mount_path.clone(),
                duration_micros,
                success: true,
                volume_guid_path: None,
                error_code: None,
            });
            return Err(error);
        }
    };
    observer(WindowsVolumeObservation::VolumeGuid {
        mount_path: mount_path.clone(),
        duration_micros,
        success: true,
        volume_guid_path: Some(volume_guid_path.clone()),
        error_code: None,
    });

    let mut filesystem_buffer = vec![0_u16; FILESYSTEM_BUFFER_UNITS];
    let ((success, api_error), duration_micros) = crate::measure_call(|| {
        // SAFETY: mount_wide is NUL-terminated; optional outputs are null as
        // documented, and the filesystem buffer length matches its allocation.
        let success = unsafe {
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
        let error = (success == 0).then(io::Error::last_os_error);
        (success, error)
    });
    if success == 0 {
        let error = api_error.expect("Win32 error captured at call boundary");
        observer(WindowsVolumeObservation::Filesystem {
            mount_path: mount_path.clone(),
            duration_micros,
            success: false,
            filesystem: None,
            error_code: error.raw_os_error(),
        });
        return Err(error);
    }
    let filesystem = match wide_to_string(&filesystem_buffer) {
        Ok(value) => value,
        Err(error) => {
            observer(WindowsVolumeObservation::Filesystem {
                mount_path: mount_path.clone(),
                duration_micros,
                success: true,
                filesystem: None,
                error_code: None,
            });
            return Err(error);
        }
    };
    observer(WindowsVolumeObservation::Filesystem {
        mount_path: mount_path.clone(),
        duration_micros,
        success: true,
        filesystem: Some(filesystem.clone()),
        error_code: None,
    });

    let mut available_bytes = 0_u64;
    let ((success, api_error), duration_micros) = crate::measure_call(|| {
        // SAFETY: target_wide is NUL-terminated and available_bytes is a valid
        // writable u64 output. The total-size outputs are optional and null.
        let success = unsafe {
            GetDiskFreeSpaceExW(
                target_wide.as_ptr(),
                &mut available_bytes,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        let error = (success == 0).then(io::Error::last_os_error);
        (success, error)
    });
    if success == 0 {
        let error = api_error.expect("Win32 error captured at call boundary");
        observer(WindowsVolumeObservation::AvailableSpace {
            target: target.to_path_buf(),
            duration_micros,
            success: false,
            available_bytes: None,
            error_code: error.raw_os_error(),
        });
        return Err(error);
    }
    observer(WindowsVolumeObservation::AvailableSpace {
        target: target.to_path_buf(),
        duration_micros,
        success: true,
        available_bytes: Some(available_bytes),
        error_code: None,
    });

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
