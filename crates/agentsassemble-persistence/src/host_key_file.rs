use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{Ed25519KeyPair, KeyPair},
};
use serde::{Deserialize, Serialize};

use crate::{
    PersistenceError,
    database_target::validate_link_count,
    private_fs::{secure_file, secure_private_directory, validate_directory, validate_file},
};

const HOST_KEY_ENVELOPE_VERSION: u32 = 2;
const MAX_PRIVATE_KEY_BYTES: u64 = 512;
const SESSION_HMAC_KEY_BYTES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostKeyPolicy {
    CreateOnly,
    CreateOrReuse,
    ReuseOnly,
}

pub(crate) struct HostKeyMaterial {
    private_key_pkcs8: Vec<u8>,
    public_key: [u8; 32],
    session_hmac_key: [u8; SESSION_HMAC_KEY_BYTES],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredHostKey {
    version: u32,
    initialization_nonce: String,
    private_key_pkcs8: String,
    session_hmac_key: String,
}

impl HostKeyMaterial {
    pub(crate) fn load_or_create(
        path: Option<&Path>,
        policy: HostKeyPolicy,
        initialization_nonce: &str,
    ) -> Result<Self, PersistenceError> {
        let Some(path) = path else {
            return Self::generate();
        };
        let parent = path.parent().ok_or(PersistenceError::InvalidHostIdentity)?;
        let allow_create = policy != HostKeyPolicy::ReuseOnly;
        ensure_private_directory(parent, allow_create)?;
        match open_existing(path) {
            Ok(_) if policy == HostKeyPolicy::CreateOnly => {
                Err(PersistenceError::InvalidHostIdentity)
            }
            Ok(file) => Self::read(file, initialization_nonce),
            Err(error) if error.kind() == io::ErrorKind::NotFound && allow_create => create_key(
                path,
                policy == HostKeyPolicy::CreateOrReuse,
                initialization_nonce,
            ),
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

    pub(crate) const fn session_hmac_key(&self) -> &[u8; SESSION_HMAC_KEY_BYTES] {
        &self.session_hmac_key
    }

    fn generate() -> Result<Self, PersistenceError> {
        let random = SystemRandom::new();
        let document = Ed25519KeyPair::generate_pkcs8(&random)
            .map_err(|_| PersistenceError::HostIdentityEntropy)?;
        let mut session_hmac_key = [0_u8; SESSION_HMAC_KEY_BYTES];
        random
            .fill(&mut session_hmac_key)
            .map_err(|_| PersistenceError::HostIdentityEntropy)?;
        Self::parse(document.as_ref().to_vec(), session_hmac_key)
    }

    fn read(mut file: File, initialization_nonce: &str) -> Result<Self, PersistenceError> {
        validate_key_file(&file)?;
        let mut payload = Vec::new();
        Read::take(&mut file, MAX_PRIVATE_KEY_BYTES + 1)
            .read_to_end(&mut payload)
            .map_err(PersistenceError::HostIdentityFile)?;
        if payload.len() as u64 > MAX_PRIVATE_KEY_BYTES {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        let stored: StoredHostKey =
            serde_json::from_slice(&payload).map_err(|_| PersistenceError::InvalidHostIdentity)?;
        if stored.version != HOST_KEY_ENVELOPE_VERSION
            || stored.initialization_nonce != initialization_nonce
        {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        let private_key_pkcs8 = URL_SAFE_NO_PAD
            .decode(stored.private_key_pkcs8.as_bytes())
            .map_err(|_| PersistenceError::InvalidHostIdentity)?;
        if URL_SAFE_NO_PAD.encode(&private_key_pkcs8) != stored.private_key_pkcs8 {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        let session_hmac_key = URL_SAFE_NO_PAD
            .decode(stored.session_hmac_key.as_bytes())
            .map_err(|_| PersistenceError::InvalidHostIdentity)?;
        if URL_SAFE_NO_PAD.encode(&session_hmac_key) != stored.session_hmac_key {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        let session_hmac_key = session_hmac_key
            .try_into()
            .map_err(|_| PersistenceError::InvalidHostIdentity)?;
        Self::parse(private_key_pkcs8, session_hmac_key)
    }

    fn parse(
        private_key_pkcs8: Vec<u8>,
        session_hmac_key: [u8; SESSION_HMAC_KEY_BYTES],
    ) -> Result<Self, PersistenceError> {
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
            session_hmac_key,
        })
    }
}

fn create_key(
    path: &Path,
    allow_existing: bool,
    initialization_nonce: &str,
) -> Result<HostKeyMaterial, PersistenceError> {
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
            if let Err(error) = write_key(&mut file, &material, initialization_nonce) {
                drop(file);
                let _ = std::fs::remove_file(path);
                return Err(PersistenceError::HostIdentityFile(error));
            }
            Ok(material)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && allow_existing => {
            HostKeyMaterial::read(
                open_existing(path).map_err(PersistenceError::HostIdentityFile)?,
                initialization_nonce,
            )
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Err(PersistenceError::InvalidHostIdentity)
        }
        Err(error) => Err(PersistenceError::HostIdentityFile(error)),
    }
}

fn write_key(
    file: &mut File,
    material: &HostKeyMaterial,
    initialization_nonce: &str,
) -> io::Result<()> {
    secure_file(file)?;
    let payload = serde_json::to_vec(&StoredHostKey {
        version: HOST_KEY_ENVELOPE_VERSION,
        initialization_nonce: initialization_nonce.to_owned(),
        private_key_pkcs8: URL_SAFE_NO_PAD.encode(material.private_key_pkcs8()),
        session_hmac_key: URL_SAFE_NO_PAD.encode(material.session_hmac_key()),
    })
    .map_err(io::Error::other)?;
    if payload.len() as u64 > MAX_PRIVATE_KEY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "host key envelope exceeds its storage limit",
        ));
    }
    file.write_all(&payload)?;
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
