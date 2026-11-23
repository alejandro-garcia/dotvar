//! Windows adapters for `storage`: file identity, DACL handling and atomic replacement.
//!
//! This is the only place in `storage` that uses FFI. Every wrapper is a minimal
//! safe function that owns the resources it acquires, never returns them raw, and
//! reports failures as `io::Error` without paths, names, or file contents.

#![allow(unsafe_code)]

use std::{
    ffi::c_void,
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_SUCCESS, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    },
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_FILE_OBJECT,
        },
        DACL_SECURITY_INFORMATION, GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
        MOVEFILE_WRITE_THROUGH, MoveFileExW, OPEN_EXISTING, READ_CONTROL,
        REPLACEFILE_WRITE_THROUGH, ReplaceFileW,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(super) use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_SYSTEM,
};

/// What `storage` needs to know about an open file: physical identity, link
/// count, access-relevant attributes, and the DACL (when readable).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileInfo {
    pub attributes: u32,
    pub volume: u32,
    pub index: u64,
    pub links: u32,
    /// SDDL text of the DACL. It describes access, never file contents.
    pub dacl: Option<String>,
}

/// Frees memory allocated by the security APIs with `LocalAlloc`.
struct LocalMemory(*mut c_void);

impl Drop for LocalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a Windows API documented to allocate
            // it with LocalAlloc, and this guard is its only owner.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    units.push(0);
    Ok(units)
}

fn wide_text(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Copy a NUL-terminated UTF-16 string returned by a Windows API.
///
/// # Safety
/// `text` must point to a valid, NUL-terminated UTF-16 string.
unsafe fn string_from_wide(text: *const u16) -> String {
    let mut length = 0;
    // SAFETY: the caller guarantees a NUL terminator, so every read stays in bounds.
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` units were just read from the same valid allocation.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) })
}

fn check(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn open_handle(
    path: &Path,
    access: u32,
    attributes: Option<*const SECURITY_ATTRIBUTES>,
    disposition: u32,
    flags: u32,
) -> io::Result<File> {
    let path = wide(path)?;
    // SAFETY: `path` is NUL-terminated and outlives the call; the security
    // attributes pointer, when present, is valid for the call.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            attributes.unwrap_or(null()),
            disposition,
            flags,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a valid handle that nothing else owns.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

/// SID of the current process user in its `S-1-...` string form.
pub(super) fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = null_mut();
    // SAFETY: `token` is a valid out pointer for the current process token.
    check(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) })?;
    // SAFETY: OpenProcessToken succeeded, so we own this handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut length = 0;
    // SAFETY: a null buffer with zero length only asks for the required size.
    unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut length) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 storage keeps the buffer aligned for TOKEN_USER.
    let mut buffer = vec![0u64; (length as usize).div_ceil(size_of::<u64>())];
    // SAFETY: the buffer is at least `length` bytes and suitably aligned.
    check(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    })?;
    // SAFETY: the buffer now holds an initialized TOKEN_USER.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = null_mut();
    // SAFETY: `sid` points into `buffer`, which is alive; `text` is a valid out pointer.
    check(unsafe { ConvertSidToStringSidW(sid, &mut text) })?;
    let _free = LocalMemory(text.cast());
    // SAFETY: the API returned a NUL-terminated UTF-16 string.
    Ok(unsafe { string_from_wide(text) })
}

/// SDDL of the DACL that grants the current user, and nobody else, full access.
/// The `P` flag blocks inheritance from the parent directory.
pub(super) fn owner_only_sddl() -> io::Result<String> {
    Ok(format!("D:P(A;;FA;;;{})", current_user_sid()?))
}

/// Create a new file whose protected DACL grants only the current user. The
/// descriptor is set at creation, so the file is never reachable by anyone
/// else, not even before the first byte is written.
pub(super) fn create_private(path: &Path) -> io::Result<File> {
    let sddl = wide_text(&owner_only_sddl()?);
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `sddl` is NUL-terminated; `descriptor` is a valid out pointer.
    check(unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    })?;
    let _free = LocalMemory(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    open_handle(
        path,
        GENERIC_READ | GENERIC_WRITE,
        Some(&attributes),
        CREATE_NEW,
        FILE_ATTRIBUTE_NORMAL,
    )
}

fn dacl_of(file: &File) -> io::Result<String> {
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: the handle is valid for the call and the out pointer is valid.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _free = LocalMemory(descriptor);
    let mut text: *mut u16 = null_mut();
    // SAFETY: `descriptor` was just returned by GetSecurityInfo and is alive.
    check(unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            null_mut(),
        )
    })?;
    let _free_text = LocalMemory(text.cast());
    // SAFETY: the API returned a NUL-terminated UTF-16 string.
    Ok(without_auto_inherit_flags(&unsafe {
        string_from_wide(text)
    }))
}

/// ReplaceFileW keeps the ACEs of the replaced file but may set or clear the
/// `AI`/`AR` bookkeeping flags of the inheritance engine. They grant nothing, so
/// they are left out to compare what the DACL allows.
fn without_auto_inherit_flags(sddl: &str) -> String {
    let Some(rest) = sddl.strip_prefix("D:") else {
        return sddl.to_owned();
    };
    let (flags, entries) = rest.split_at(rest.find('(').unwrap_or(rest.len()));
    format!("D:{}{entries}", flags.replace("AI", "").replace("AR", ""))
}

/// Identity, link count, attributes and DACL of an open file. A DACL that cannot
/// be read is reported as `None` so callers can treat access as unverifiable.
pub(super) fn file_info(file: &File) -> io::Result<FileInfo> {
    // SAFETY: BY_HANDLE_FILE_INFORMATION is plain data; all-zero is valid.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is valid for the call and `info` is a valid out pointer.
    check(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) })?;
    Ok(FileInfo {
        attributes: info.dwFileAttributes,
        volume: info.dwVolumeSerialNumber,
        index: u64::from(info.nFileIndexHigh) << 32 | u64::from(info.nFileIndexLow),
        links: info.nNumberOfLinks,
        dacl: dacl_of(file).ok(),
    })
}

/// Like [`file_info`] for a path. With `follow` false, a symlink is described
/// itself instead of its target.
pub(super) fn path_info(path: &Path, follow: bool) -> io::Result<FileInfo> {
    let reparse = if follow {
        0
    } else {
        FILE_FLAG_OPEN_REPARSE_POINT
    };
    let file = open_handle(
        path,
        READ_CONTROL | FILE_READ_ATTRIBUTES,
        None,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | reparse,
    )?;
    file_info(&file)
}

/// Atomically swap `replacement` in for `replaced`, keeping the replaced file's
/// ACL, attributes and alternate streams. Fails, leaving `replaced` untouched,
/// when those cannot be merged. The data is written through before returning.
pub(super) fn replace_file(replaced: &Path, replacement: &Path) -> io::Result<()> {
    let replaced = wide(replaced)?;
    let replacement = wide(replacement)?;
    // SAFETY: both paths are NUL-terminated and outlive the call; the optional
    // backup, exclude and reserved arguments are null as documented.
    check(unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            null(),
            REPLACEFILE_WRITE_THROUGH,
            null(),
            null(),
        )
    })
}

/// Move `from` to `to`, failing with `AlreadyExists` instead of replacing it.
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from = wide(from)?;
    let to = wide(to)?;
    // SAFETY: both paths are NUL-terminated and outlive the call. Without
    // MOVEFILE_REPLACE_EXISTING an existing destination makes the call fail.
    check(unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) })
}
