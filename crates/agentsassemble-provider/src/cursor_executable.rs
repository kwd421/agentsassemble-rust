use std::{io, path::Path};

use super::{BoundExecutable, FilesystemFailure};

pub(crate) async fn cursor_executable_identity(path: String) -> Result<String, FilesystemFailure> {
    super::run_bounded(move || identity(Path::new(&path))).await
}

pub(crate) async fn bind_cursor_executable(
    path: String,
    expected_identity: String,
) -> Result<BoundExecutable, FilesystemFailure> {
    super::run_bounded(move || {
        let path = Path::new(&path);
        #[cfg(unix)]
        if let Some(package) =
            agentsassemble_domain::cursor_package::CursorExecutablePackage::inspect(path)?
        {
            return package::bind(package, &expected_identity);
        }
        let mut bound = super::bind_executable_sync(path, &expected_identity)?;
        bound.allows_child_processes = true;
        Ok(bound)
    })
    .await
}

fn identity(path: &Path) -> io::Result<String> {
    #[cfg(unix)]
    if let Some(package) =
        agentsassemble_domain::cursor_package::CursorExecutablePackage::inspect(path)?
    {
        return Ok(package.identity());
    }
    super::executable_identity_sync(path)
}

#[cfg(unix)]
mod package {
    use std::{
        fs::{self, File, OpenOptions},
        io::{self, Read},
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    use agentsassemble_domain::cursor_package::CursorExecutablePackage;
    use same_file::Handle;

    use super::super::{BoundExecutable, ExecutableStaging};

    pub(super) fn bind(
        manifest: CursorExecutablePackage,
        expected: &str,
    ) -> io::Result<BoundExecutable> {
        if manifest.identity() != expected {
            return Err(io::Error::other("Cursor package identity changed"));
        }
        let staging = ExecutableStaging::create()?;
        let destination = staging.path().join("package");
        fs::create_dir(&destination)?;
        for member in &manifest.members {
            let mut source = manifest.open_member(member)?;
            let handle = Handle::from_file(source.try_clone()?)?;
            let target = destination.join(&member.relative);
            fs::create_dir_all(
                target
                    .parent()
                    .ok_or_else(|| io::Error::other("Cursor member parent missing"))?,
            )?;
            let mut staged = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(&target)?;
            if io::copy(&mut (&mut source).take(member.bytes + 1), &mut staged)? != member.bytes {
                return Err(io::Error::other(
                    "Cursor package size changed during staging",
                ));
            }
            staged.sync_all()?;
            super::super::verify_staged_identity(&handle, &member.identity, &mut staged)?;
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if member.executable { 0o500 } else { 0o400 }),
            )?;
        }
        let launch_path = destination.join(manifest.entry);
        let file = File::open(&launch_path)?;
        Ok(BoundExecutable {
            file,
            launch_path: launch_path
                .to_str()
                .ok_or_else(|| io::Error::other("Cursor staged path is not UTF-8"))?
                .to_owned(),
            companion_files: Vec::new(),
            allows_child_processes: true,
            _staging: Some(staging),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            inherited_executable_fd: false,
        })
    }
}

#[cfg(all(test, unix))]
#[path = "cursor_executable_tests.rs"]
mod tests;
