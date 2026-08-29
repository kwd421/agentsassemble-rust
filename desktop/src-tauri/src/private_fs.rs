#![cfg(windows)]

use std::{fs::File, io, path::Path};

use winapi::um::winnt::{CONTAINER_INHERIT_ACE, FILE_ALL_ACCESS, OBJECT_INHERIT_ACE, PSID};
use windows_acl::acl::{ACL, AceType};
use windows_permissions::{
    LocalBox, SecurityDescriptor,
    constants::{SeObjectType, SecurityInformation},
};

pub(crate) fn create_private_directory(path: &Path) -> io::Result<()> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("path is not valid Unicode"))?;
    let (_, current) = current_sid()?;
    let descriptor: LocalBox<SecurityDescriptor> =
        format!("O:{current}D:P(A;OICI;FA;;;{current})").parse()?;
    let owner = descriptor
        .owner()
        .ok_or_else(|| io::Error::other("private descriptor has no owner"))?;
    let dacl = descriptor
        .dacl()
        .ok_or_else(|| io::Error::other("private descriptor has no DACL"))?;
    let mut native = winsafe::InitializeSecurityDescriptor().map_err(winsafe_error)?;
    native.Owner = std::ptr::from_ref(owner).cast_mut().cast();
    native.Dacl = std::ptr::from_ref(dacl).cast_mut().cast::<winsafe::ACL>();
    native.Control = winsafe::co::SE::DACL_PRESENT | winsafe::co::SE::DACL_PROTECTED;
    if !winsafe::IsValidSecurityDescriptor(&native) {
        return Err(io::Error::other("private security descriptor is invalid"));
    }
    let mut attributes = winsafe::SECURITY_ATTRIBUTES::default();
    attributes.set_lpSecurityDescriptor(Some(&mut native));
    winsafe::CreateDirectory(path, Some(&attributes)).map_err(winsafe_error)
}

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

pub(crate) fn validate_private_directory_handle(file: &File) -> io::Result<()> {
    let descriptor = windows_permissions::wrappers::GetSecurityInfo(
        file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Owner,
    )?;
    let (_, current) = current_sid()?;
    let owned_by_current_user = descriptor
        .owner()
        .is_some_and(|owner| owner.to_string() == current);
    let acl = acl_from_handle(file)?;
    if owned_by_current_user && validate(&acl, true)? {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows directory is not owner-only",
        ))
    }
}

fn secure_handle(file: &File, inheritable: bool) -> io::Result<()> {
    harden(acl_from_handle(file)?, inheritable)
}

fn acl_from_handle(file: &File) -> io::Result<ACL> {
    use std::os::windows::io::AsRawHandle;

    ACL::from_file_handle(file.as_raw_handle().cast::<winapi::ctypes::c_void>(), false)
        .map_err(windows_error)
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
    if validate(&acl, inheritable)? {
        Ok(())
    } else {
        Err(io::Error::other("Windows DACL did not become owner-only"))
    }
}

fn validate(acl: &ACL, inheritable: bool) -> io::Result<bool> {
    let (_, current) = current_sid()?;
    let entries = acl.all().map_err(windows_error)?;
    let required_flags = if inheritable {
        CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE
    } else {
        0
    };
    Ok(entries.len() == 1
        && entries[0].entry_type == AceType::AccessAllow
        && entries[0].string_sid == current
        && entries[0].flags & required_flags == required_flags
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

fn winsafe_error(error: winsafe::co::ERROR) -> io::Error {
    windows_error(error.raw())
}
