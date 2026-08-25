use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};

use crate::{
    PersistenceError,
    database_target::validate_link_count,
    private_fs::{secure_file, secure_private_directory, validate_directory, validate_file},
};

const MAX_PRIVATE_KEY_BYTES: u64 = 512;

pub(crate) struct HostKeyMaterial {
    private_key_pkcs8: Vec<u8>,
    public_key: [u8; 32],
}

impl HostKeyMaterial {
    pub(crate) fn load_or_create(
        path: Option<&Path>,
        allow_create: bool,
    ) -> Result<Self, PersistenceError> {
        let Some(path) = path else {
            return Self::generate();
        };
        let parent = path.parent().ok_or(PersistenceError::InvalidHostIdentity)?;
        ensure_private_directory(parent, allow_create)?;
        match open_existing(path) {
            Ok(file) => Self::read(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound && allow_create => {
                create_key(path)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(PersistenceError::HostIdentityMissing)
            }
            Err(error) => Err(PersistenceError::HostIdentityFile(error)),
        }
    }

    pub(crate) const fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }

    pub(crate) fn private_key_pkcs8(&self) -> &[u8] {
        &self.private_key_pkcs8
    }

    fn generate() -> Result<Self, PersistenceError> {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| PersistenceError::HostIdentityEntropy)?;
        Self::parse(document.as_ref().to_vec())
    }

    fn read(mut file: File) -> Result<Self, PersistenceError> {
        validate_key_file(&file)?;
        let mut payload = Vec::new();
        Read::take(&mut file, MAX_PRIVATE_KEY_BYTES + 1)
            .read_to_end(&mut payload)
            .map_err(PersistenceError::HostIdentityFile)?;
        if payload.len() as u64 > MAX_PRIVATE_KEY_BYTES {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        Self::parse(payload)
    }

    fn parse(private_key_pkcs8: Vec<u8>) -> Result<Self, PersistenceError> {
        let key_pair = Ed25519KeyPair::from_pkcs8(&private_key_pkcs8)
            .map_err(|_| PersistenceError::InvalidHostIdentity)?;
        let public_key = key_pair
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| PersistenceError::InvalidHostIdentity)?;
        Ok(Self {
            private_key_pkcs8,
            public_key,
        })
    }
}

fn create_key(path: &Path) -> Result<HostKeyMaterial, PersistenceError> {
    let material = HostKeyMaterial::generate()?;
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            if let Err(error) = write_key(&mut file, material.private_key_pkcs8()) {
                drop(file);
                let _ = std::fs::remove_file(path);
                return Err(PersistenceError::HostIdentityFile(error));
            }
            Ok(material)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            HostKeyMaterial::read(open_existing(path).map_err(PersistenceError::HostIdentityFile)?)
        }
        Err(error) => Err(PersistenceError::HostIdentityFile(error)),
    }
}

fn write_key(file: &mut File, payload: &[u8]) -> io::Result<()> {
    secure_file(file)?;
    file.write_all(payload)?;
    file.sync_all()
}

fn open_existing(path: &Path) -> io::Result<File> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "host signing key may not be a symbolic link",
        ));
    }
    OpenOptions::new().read(true).open(path)
}

fn validate_key_file(file: &File) -> Result<(), PersistenceError> {
    let metadata = file
        .metadata()
        .map_err(PersistenceError::HostIdentityFile)?;
    if !metadata.is_file() || metadata.len() > MAX_PRIVATE_KEY_BYTES {
        return Err(PersistenceError::InvalidHostIdentity);
    }
    validate_link_count(file, &metadata)?;
    if !validate_file(file).map_err(PersistenceError::HostIdentityFile)? {
        return Err(PersistenceError::InvalidHostIdentity);
    }
    Ok(())
}

fn ensure_private_directory(path: &Path, allow_create: bool) -> Result<(), PersistenceError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound && allow_create => {
            std::fs::create_dir(path).map_err(PersistenceError::HostIdentityFile)?;
            secure_private_directory(path).map_err(PersistenceError::HostIdentityFile)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(PersistenceError::HostIdentityMissing);
        }
        Err(error) => return Err(PersistenceError::HostIdentityFile(error)),
    }
    if validate_directory(path).map_err(PersistenceError::HostIdentityFile)? {
        Ok(())
    } else {
        Err(PersistenceError::InvalidHostIdentity)
    }
}
