use std::{fs::File, io, path::Path};

#[cfg(unix)]
pub(crate) fn validate_directory(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    Ok(path.metadata()?.permissions().mode() & 0o022 == 0)
}

#[cfg(unix)]
/// Restricts a directory to the current OS user.
///
/// # Errors
///
/// Returns the platform permission error when the directory cannot be secured.
pub fn secure_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
pub(crate) fn secure_file(file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(std::fs::Permissions::from_mode(0o600))
}

#[cfg(unix)]
pub(crate) fn validate_file(file: &File) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    Ok(file.metadata()?.permissions().mode().trailing_zeros() >= 6)
}

#[cfg(windows)]
pub(crate) fn validate_directory(path: &Path) -> io::Result<bool> {
    validate_acl(&acl_from_path(path)?)
}

#[cfg(windows)]
/// Restricts a directory to the current Windows user and verifies the resulting DACL.
///
/// # Errors
///
/// Returns an ACL error when the directory cannot be made owner-only.
pub fn secure_private_directory(path: &Path) -> io::Result<()> {
    harden_acl(acl_from_path(path)?, true)
}

#[cfg(windows)]
pub(crate) fn secure_file(file: &File) -> io::Result<()> {
    harden_acl(acl_from_file(file)?, false)
}

#[cfg(windows)]
pub(crate) fn validate_file(file: &File) -> io::Result<bool> {
    validate_acl(&acl_from_file(file)?)
}

#[cfg(windows)]
fn acl_from_path(path: &Path) -> io::Result<windows_acl::acl::ACL> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("path is not valid Unicode"))?;
    windows_acl::acl::ACL::from_file_path(path, false).map_err(windows_error)
}

#[cfg(windows)]
fn acl_from_file(file: &File) -> io::Result<windows_acl::acl::ACL> {
    use std::os::windows::io::AsRawHandle;

    windows_acl::acl::ACL::from_file_handle(
        file.as_raw_handle().cast::<winapi::ctypes::c_void>(),
        false,
    )
    .map_err(windows_error)
}

#[cfg(windows)]
fn current_sid() -> io::Result<(Vec<u8>, String)> {
    use winapi::um::winnt::PSID;

    let user = windows_acl::helper::current_user()
        .ok_or_else(|| io::Error::other("cannot identify the current Windows user"))?;
    let sid = windows_acl::helper::name_to_sid(&user, None).map_err(windows_error)?;
    let label = windows_acl::helper::sid_to_string(sid.as_ptr() as PSID).map_err(windows_error)?;
    Ok((sid, label))
}

#[cfg(windows)]
fn harden_acl(mut acl: windows_acl::acl::ACL, inheritable: bool) -> io::Result<()> {
    use winapi::um::winnt::{FILE_ALL_ACCESS, PSID};

    let entries = acl.all().map_err(windows_error)?;
    for label in entries
        .into_iter()
        .map(|entry| entry.string_sid)
        .collect::<std::collections::BTreeSet<_>>()
    {
        let sid = windows_acl::helper::string_to_sid(&label).map_err(windows_error)?;
        acl.remove(sid.as_ptr() as PSID, None, None)
            .map_err(windows_error)?;
    }
    let (sid, _) = current_sid()?;
    acl.allow(sid.as_ptr() as PSID, inheritable, FILE_ALL_ACCESS)
        .map_err(windows_error)?;
    if validate_acl(&acl)? {
        Ok(())
    } else {
        Err(io::Error::other("Windows DACL did not become owner-only"))
    }
}

#[cfg(windows)]
fn validate_acl(acl: &windows_acl::acl::ACL) -> io::Result<bool> {
    use winapi::um::winnt::FILE_ALL_ACCESS;
    use windows_acl::acl::AceType;

    let (_, current) = current_sid()?;
    let entries = acl.all().map_err(windows_error)?;
    Ok(entries.len() == 1
        && entries[0].entry_type == AceType::AccessAllow
        && entries[0].string_sid == current
        && entries[0].mask & FILE_ALL_ACCESS == FILE_ALL_ACCESS)
}

#[cfg(windows)]
fn windows_error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}

#[cfg(all(not(unix), not(windows)))]
compile_error!("private authority permissions require Unix modes or Windows DACLs");
