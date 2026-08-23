use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use fs2::FileExt;
use rustix::fs::{AtFlags, Mode, OFlags};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::runtime::DriverError;

const HOOK_NAME: &str = "agentsassemble-room-requests";
const HOOKS_FILE: &str = "hooks.json";
const LOCK_FILE: &str = ".agentsassemble-hooks.lock";
const MAX_HOOKS_BYTES: u64 = 64 * 1024;

struct Registration {
    count: usize,
    definition: Value,
}

static REGISTRATIONS: OnceLock<Mutex<HashMap<PathBuf, Registration>>> = OnceLock::new();

pub(crate) struct AntigravityHookRegistration {
    workspace: PathBuf,
}

impl AntigravityHookRegistration {
    pub(crate) fn register(workspace: &Path) -> Result<Self, DriverError> {
        let workspace = workspace.to_path_buf();
        let definition = hook_definition();
        let registry = REGISTRATIONS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut registrations = registry.lock().map_err(|_| hook_error())?;
        if let Some(active) = registrations.get_mut(&workspace) {
            if active.definition != definition {
                return Err(hook_error());
            }
            active.count = active.count.checked_add(1).ok_or_else(hook_error)?;
            return Ok(Self { workspace });
        }
        let directory = HookDirectory::open(&workspace)?;
        let _lock = directory.lock()?;
        let mut document = directory.read_document()?;
        if !document.is_empty() {
            return Err(hook_error());
        }
        document.insert(HOOK_NAME.to_owned(), definition.clone());
        directory.write_document(&document)?;
        registrations.insert(
            workspace.clone(),
            Registration {
                count: 1,
                definition,
            },
        );
        Ok(Self { workspace })
    }
}

impl Drop for AntigravityHookRegistration {
    fn drop(&mut self) {
        let Some(registry) = REGISTRATIONS.get() else {
            return;
        };
        let Ok(mut registrations) = registry.lock() else {
            return;
        };
        let Some(active) = registrations.get_mut(&self.workspace) else {
            return;
        };
        if active.count > 1 {
            active.count -= 1;
            return;
        }
        let definition = active.definition.clone();
        registrations.remove(&self.workspace);
        let Ok(directory) = HookDirectory::open(&self.workspace) else {
            return;
        };
        let Ok(_lock) = directory.lock() else {
            return;
        };
        let Ok(mut document) = directory.read_document() else {
            return;
        };
        if document.get(HOOK_NAME) != Some(&definition) {
            return;
        }
        document.remove(HOOK_NAME);
        let _ = directory.write_document(&document);
    }
}

fn hook_definition() -> Value {
    json!({
        "PreToolUse": [{
            "matcher": "run_command|ask_permission|ask_question",
            "hooks": [{
                "type": "command",
                "command": "agentsassemble-room hook",
                "timeout": 900
            }]
        }]
    })
}

struct HookDirectory {
    directory: File,
}

impl HookDirectory {
    fn open(workspace: &Path) -> Result<Self, DriverError> {
        let workspace = File::open(workspace).map_err(|_| hook_error())?;
        if !workspace.metadata().map_err(|_| hook_error())?.is_dir() {
            return Err(hook_error());
        }
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let agents = match rustix::fs::openat(&workspace, ".agents", flags, Mode::empty()) {
            Ok(directory) => directory,
            Err(rustix::io::Errno::NOENT) => {
                rustix::fs::mkdirat(&workspace, ".agents", Mode::RUSR | Mode::WUSR | Mode::XUSR)
                    .map_err(|_| hook_error())?;
                rustix::fs::openat(&workspace, ".agents", flags, Mode::empty())
                    .map_err(|_| hook_error())?
            }
            Err(_) => return Err(hook_error()),
        };
        let directory = File::from(agents);
        if !directory.metadata().map_err(|_| hook_error())?.is_dir() {
            return Err(hook_error());
        }
        Ok(Self { directory })
    }

    fn lock(&self) -> Result<File, DriverError> {
        let flags = OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let descriptor =
            rustix::fs::openat(&self.directory, LOCK_FILE, flags, Mode::RUSR | Mode::WUSR)
                .map_err(|_| hook_error())?;
        let file = File::from(descriptor);
        file.lock_exclusive().map_err(|_| hook_error())?;
        Ok(file)
    }

    fn read_document(&self) -> Result<Map<String, Value>, DriverError> {
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let descriptor = match rustix::fs::openat(&self.directory, HOOKS_FILE, flags, Mode::empty())
        {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT) => return Ok(Map::new()),
            Err(_) => return Err(hook_error()),
        };
        let file = File::from(descriptor);
        let metadata = file.metadata().map_err(|_| hook_error())?;
        if !metadata.is_file() || metadata.len() > MAX_HOOKS_BYTES {
            return Err(hook_error());
        }
        let mut encoded = String::new();
        file.take(MAX_HOOKS_BYTES + 1)
            .read_to_string(&mut encoded)
            .map_err(|_| hook_error())?;
        if encoded.len() as u64 > MAX_HOOKS_BYTES {
            return Err(hook_error());
        }
        let document: Value = serde_json::from_str(&encoded).map_err(|_| hook_error())?;
        document.as_object().cloned().ok_or_else(hook_error)
    }

    fn write_document(&self, document: &Map<String, Value>) -> Result<(), DriverError> {
        if document.is_empty() {
            match rustix::fs::unlinkat(&self.directory, HOOKS_FILE, AtFlags::empty()) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => {
                    rustix::fs::fsync(&self.directory).map_err(|_| hook_error())?;
                    return Ok(());
                }
                Err(_) => return Err(hook_error()),
            }
        }
        let encoded =
            serde_json::to_vec(&Value::Object(document.clone())).map_err(|_| hook_error())?;
        if encoded.len() as u64 > MAX_HOOKS_BYTES {
            return Err(hook_error());
        }
        let temporary = format!(".hooks.{}.tmp", Uuid::new_v4().simple());
        let flags =
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let descriptor = rustix::fs::openat(
            &self.directory,
            temporary.as_str(),
            flags,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|_| hook_error())?;
        let mut file = File::from(descriptor);
        let write = file
            .write_all(&encoded)
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all());
        drop(file);
        if write.is_err() {
            let _ = rustix::fs::unlinkat(&self.directory, temporary.as_str(), AtFlags::empty());
            return Err(hook_error());
        }
        if rustix::fs::renameat(
            &self.directory,
            temporary.as_str(),
            &self.directory,
            HOOKS_FILE,
        )
        .is_err()
        {
            let _ = rustix::fs::unlinkat(&self.directory, temporary.as_str(), AtFlags::empty());
            return Err(hook_error());
        }
        rustix::fs::fsync(&self.directory).map_err(|_| hook_error())
    }
}

const fn hook_error() -> DriverError {
    DriverError::new(
        "provider_hook_unavailable",
        "The Antigravity workspace hook could not be installed safely.",
    )
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{AntigravityHookRegistration, HOOK_NAME};

    #[test]
    fn registration_rejects_existing_project_hooks() {
        let workspace =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create hook workspace: {error}"));
        let directory = workspace.path().join(".agents");
        std::fs::create_dir(&directory)
            .unwrap_or_else(|error| panic!("create hook directory: {error}"));
        let mut document = serde_json::Map::new();
        document.insert(
            "project-hook".to_owned(),
            serde_json::json!({"custom": true}),
        );
        std::fs::write(
            directory.join("hooks.json"),
            serde_json::to_vec(&document).unwrap_or_default(),
        )
        .unwrap_or_else(|error| panic!("write previous hook: {error}"));
        assert!(AntigravityHookRegistration::register(workspace.path()).is_err());
    }

    #[test]
    fn registration_owns_one_managed_hook_and_removes_it_on_last_drop() {
        let workspace =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create hook workspace: {error}"));
        let directory = workspace.path().join(".agents");
        {
            let first = AntigravityHookRegistration::register(workspace.path())
                .unwrap_or_else(|error| panic!("register first hook: {error}"));
            let second = AntigravityHookRegistration::register(workspace.path())
                .unwrap_or_else(|error| panic!("register second hook: {error}"));
            drop(first);
            let installed: Value = serde_json::from_slice(
                &std::fs::read(directory.join("hooks.json"))
                    .unwrap_or_else(|error| panic!("read installed hook: {error}")),
            )
            .unwrap_or_else(|error| panic!("decode installed hook: {error}"));
            assert_eq!(
                installed[HOOK_NAME]["PreToolUse"][0]["hooks"][0]["command"],
                "agentsassemble-room hook"
            );
            drop(second);
        }
        assert!(!directory.join("hooks.json").exists());
    }
}
