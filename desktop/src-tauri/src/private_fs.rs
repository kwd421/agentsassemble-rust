#![cfg(windows)]

use std::{fs::File, io, path::Path};

use winapi::um::winnt::{FILE_ALL_ACCESS, PSID};
use windows_acl::acl::{ACL, AceType};

pub(crate) fn secure_directory(path: &Path) -> io::Result<()> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("path is not valid Unicode"))?;
    harden(
        ACL::from_file_path(path, false).map_err(windows_error)?,
        true,
    )
}

pub(crate) fn secure_file(file: &File) -> io::Result<()> {
    secure_handle(file, false)
}

pub(crate) fn secure_directory_handle(file: &File) -> io::Result<()> {
    secure_handle(file, true)
}

fn secure_handle(file: &File, inheritable: bool) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    harden(
        ACL::from_file_handle(file.as_raw_handle().cast::<winapi::ctypes::c_void>(), false)
            .map_err(windows_error)?,
        inheritable,
    )
}

fn harden(mut acl: ACL, inheritable: bool) -> io::Result<()> {
    let labels = acl
        .all()
        .map_err(windows_error)?
        .into_iter()
        .map(|entry| entry.string_sid)
        .collect::<std::collections::BTreeSet<_>>();
    for label in labels {
        let sid = windows_acl::helper::string_to_sid(&label).map_err(windows_error)?;
        acl.remove(sid.as_ptr() as PSID, None, None)
            .map_err(windows_error)?;
    }
    let (sid, _) = current_sid()?;
    acl.allow(sid.as_ptr() as PSID, inheritable, FILE_ALL_ACCESS)
        .map_err(windows_error)?;
    if validate(&acl)? {
        Ok(())
    } else {
        Err(io::Error::other("Windows DACL did not become owner-only"))
    }
}

fn validate(acl: &ACL) -> io::Result<bool> {
    let (_, current) = current_sid()?;
    let entries = acl.all().map_err(windows_error)?;
    Ok(entries.len() == 1
        && entries[0].entry_type == AceType::AccessAllow
        && entries[0].string_sid == current
        && entries[0].mask & FILE_ALL_ACCESS == FILE_ALL_ACCESS)
}

fn current_sid() -> io::Result<(Vec<u8>, String)> {
    let user = windows_acl::helper::current_user()
        .ok_or_else(|| io::Error::other("cannot identify the current Windows user"))?;
    let sid = windows_acl::helper::name_to_sid(&user, None).map_err(windows_error)?;
    let label = windows_acl::helper::sid_to_string(sid.as_ptr() as PSID).map_err(windows_error)?;
    Ok((sid, label))
}

fn windows_error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}
