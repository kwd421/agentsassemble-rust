use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
};

use goblin::mach::{Mach, MachO, SingleArch};
use same_file::Handle;

use super::BoundExecutable;

const MAX_IMAGES: usize = 64;
const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

// Node's Homebrew Mach-O host has loader-relative libnode dependencies. Keep the
// declared layout and byte custody together instead of launching its mutable source.
pub(super) fn bind(source: &Path, identity: &str) -> io::Result<BoundExecutable> {
    let mut bound = super::bind_executable_sync(source, identity)?;
    let old = PathBuf::from(&bound.launch_path);
    let root = old.parent().ok_or_else(invalid)?.to_path_buf();
    let entry = root.join("image/bin/provider");
    fs::create_dir_all(entry.parent().ok_or_else(invalid)?)?;
    fs::rename(&old, &entry)?;
    let mut queue = VecDeque::from([(source.to_path_buf(), entry.clone())]);
    let mut staged = BTreeMap::from([(entry.clone(), source.to_path_buf())]);
    while let Some((original, copied)) = queue.pop_front() {
        let (libraries, rpaths) = dependencies(&copied)?;
        for library in libraries {
            if Path::new(&library).is_absolute() {
                continue;
            }
            let (dependency, destination) = resolve(&original, &copied, &library, &rpaths)?;
            // Absolute rpaths retain their existing system/package loader contract.
            let Some(destination) = destination else {
                continue;
            };
            let destination = confined(&root, &destination)?;
            let dependency = dependency.canonicalize()?;
            if let Some(previous) = staged.get(&destination) {
                if previous != &dependency {
                    return Err(invalid());
                }
                continue;
            }
            if staged.len() >= MAX_IMAGES {
                return Err(invalid());
            }
            copy_dependency(&dependency, &destination)?;
            staged.insert(destination.clone(), dependency.clone());
            queue.push_back((dependency, destination));
        }
    }
    entry
        .to_str()
        .ok_or_else(invalid)?
        .clone_into(&mut bound.launch_path);
    bound.allows_child_processes = true;
    Ok(bound)
}

fn dependencies(path: &Path) -> io::Result<(Vec<String>, Vec<String>)> {
    if fs::metadata(path)?.len() > MAX_IMAGE_BYTES {
        return Err(invalid());
    }
    let bytes = fs::read(path)?;
    let parsed = Mach::parse(&bytes).map_err(|_| invalid())?;
    let image = match &parsed {
        Mach::Binary(image) => return Ok(names(image)),
        Mach::Fat(images) => {
            #[cfg(target_arch = "aarch64")]
            let cpu = goblin::mach::constants::cputype::CPU_TYPE_ARM64;
            #[cfg(target_arch = "x86_64")]
            let cpu = goblin::mach::constants::cputype::CPU_TYPE_X86_64;
            images
                .find(|arch| arch.is_ok_and(|arch| arch.cputype == cpu))
                .ok_or_else(invalid)?
                .map_err(|_| invalid())?
        }
    };
    match image {
        SingleArch::MachO(image) => Ok(names(&image)),
        SingleArch::Archive(_) => Err(invalid()),
    }
}

fn names(image: &MachO<'_>) -> (Vec<String>, Vec<String>) {
    (
        image
            .libs
            .iter()
            .skip(1)
            .map(|name| (*name).to_owned())
            .collect(),
        image.rpaths.iter().map(|name| (*name).to_owned()).collect(),
    )
}

fn resolve(
    source: &Path,
    copied: &Path,
    library: &str,
    rpaths: &[String],
) -> io::Result<(PathBuf, Option<PathBuf>)> {
    let source_dir = source.parent().ok_or_else(invalid)?;
    let copied_dir = copied.parent().ok_or_else(invalid)?;
    if let Some(relative) = library.strip_prefix("@loader_path/") {
        return Ok((source_dir.join(relative), Some(copied_dir.join(relative))));
    }
    let name = library.strip_prefix("@rpath/").ok_or_else(invalid)?;
    for rpath in rpaths {
        if rpath == "@loader_path" || rpath.starts_with("@loader_path/") {
            let relative = rpath
                .strip_prefix("@loader_path")
                .ok_or_else(invalid)?
                .trim_start_matches('/');
            let dependency = source_dir.join(relative).join(name);
            if dependency.is_file() {
                return Ok((dependency, Some(copied_dir.join(relative).join(name))));
            }
        } else if Path::new(rpath).is_absolute() {
            let dependency = Path::new(rpath).join(name);
            if dependency.is_file() {
                return Ok((dependency, None));
            }
        } else {
            return Err(invalid());
        }
    }
    Err(invalid())
}

fn confined(root: &Path, destination: &Path) -> io::Result<PathBuf> {
    let mut result = root.to_path_buf();
    for part in destination
        .strip_prefix(root)
        .map_err(|_| invalid())?
        .components()
    {
        match part {
            Component::Normal(name) => result.push(name),
            Component::CurDir => {}
            Component::ParentDir if result != root => {
                result.pop();
            }
            _ => return Err(invalid()),
        }
    }
    if result == root {
        return Err(invalid());
    }
    Ok(result)
}

fn copy_dependency(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source_file = File::open(source)?;
    let metadata = source_file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
        return Err(invalid());
    }
    let handle = Handle::from_file(source_file.try_clone()?)?;
    let identity = agentsassemble_domain::stable_content_identity(&handle, &mut source_file)?;
    fs::create_dir_all(destination.parent().ok_or_else(invalid)?)?;
    let mut staged = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(destination)?;
    super::copy_and_verify_staged(&mut source_file, &handle, &identity, &mut staged)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o400))?;
    Ok(())
}

fn invalid() -> io::Error {
    io::Error::other("SDK host loader dependency could not be bound")
}

#[cfg(test)]
#[path = "macho_host_tests.rs"]
mod tests;
