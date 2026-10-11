//! Versioned, bounded file envelope. Only a reconstructed `SQLite` payload belongs
//! here; decoding bytes never makes the supplied database executable authority.
use std::io::{Read, Seek, SeekFrom, Write};

use libsodium_rs::{crypto_pwhash, crypto_secretstream::xchacha20poly1305 as stream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"AASERVER";
const CHUNK: usize = 64 * 1024;
const MAX_DATABASE_BYTES: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("backup I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported or damaged backup")]
    Invalid,
    #[error("backup passphrase is required or the file is damaged")]
    Authentication,
    #[error("backup encryption is unavailable")]
    Crypto,
    #[error("backup manifest is invalid")]
    Manifest,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KdfParameters {
    pub operations: u64,
    pub memory_bytes: usize,
}

impl Default for KdfParameters {
    fn default() -> Self {
        Self {
            operations: 2,
            memory_bytes: 64 * 1024 * 1024,
        }
    }
}

impl KdfParameters {
    fn validate(self) -> Result<(), BackupError> {
        if !(2..=4).contains(&self.operations)
            || !(64 * 1024 * 1024..=256 * 1024 * 1024).contains(&self.memory_bytes)
        {
            return Err(BackupError::Invalid);
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    major: u16,
    minor: u16,
    encryption: Option<Encryption>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Encryption {
    kdf: KdfParameters,
    salt: [u8; crypto_pwhash::SALTBYTES],
    stream_header: [u8; stream::HEADERBYTES],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub schema_version: i64,
    pub server_id: String,
    pub server_name: String,
    pub issuer: String,
    pub owner_person_id: String,
    pub source_epoch: String,
    pub source_key_fingerprint: String,
    pub snapshot_at: chrono::DateTime<chrono::Utc>,
    pub rooms: u64,
    pub messages: u64,
    pub assets: u64,
    pub database_bytes: u64,
    pub database_sha256: [u8; 32],
}

impl BackupManifest {
    fn validate(&self) -> Result<(), BackupError> {
        if self.schema_version != crate::schema_version::CURRENT_SCHEMA_VERSION
            || uuid::Uuid::parse_str(&self.server_id).is_err()
            || self.server_name.len() > 400
            || self.issuer.len() > 2048
            || self.owner_person_id.is_empty()
            || self.owner_person_id.len() > 256
            || self.source_epoch.is_empty()
            || self.source_epoch.len() > 128
            || self.source_key_fingerprint.len() != 43
            || !(512..=MAX_DATABASE_BYTES).contains(&self.database_bytes)
        {
            return Err(BackupError::Manifest);
        }
        Ok(())
    }
}

fn key(
    passphrase: &[u8],
    parameters: KdfParameters,
    salt: &[u8],
) -> Result<stream::Key, BackupError> {
    parameters.validate()?;
    if passphrase.is_empty() || passphrase.len() > 4096 {
        return Err(BackupError::Authentication);
    }
    libsodium_rs::ensure_init().map_err(|_| BackupError::Crypto)?;
    let bytes = Zeroizing::new(
        crypto_pwhash::pwhash(
            stream::KEYBYTES,
            passphrase,
            salt,
            parameters.operations,
            parameters.memory_bytes,
            crypto_pwhash::ALG_ARGON2ID13,
        )
        .map_err(|_| BackupError::Crypto)?,
    );
    stream::Key::from_bytes(&bytes).map_err(|_| BackupError::Crypto)
}

fn write_record(output: &mut impl Write, bytes: &[u8]) -> Result<(), BackupError> {
    let length = u32::try_from(bytes.len()).map_err(|_| BackupError::Invalid)?;
    output.write_all(&length.to_le_bytes())?;
    output.write_all(bytes)?;
    Ok(())
}

fn read_record(input: &mut impl Read, limit: usize) -> Result<Vec<u8>, BackupError> {
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > limit {
        return Err(BackupError::Invalid);
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Hash a stable reconstructed payload without loading it into RAM.
/// # Errors
/// Reports I/O, unsupported size, and seek failures.
pub fn measure_payload(input: &mut (impl Read + Seek)) -> Result<(u64, [u8; 32]), BackupError> {
    input.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut buffer = Zeroizing::new(vec![0; CHUNK]);
    let mut size = 0_u64;
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_DATABASE_BYTES {
            return Err(BackupError::Invalid);
        }
        hash.update(&buffer[..n]);
    }
    input.seek(SeekFrom::Start(0))?;
    Ok((size, hash.finalize().into()))
}

/// Write a complete envelope. A caller must verify and atomically publish its
/// private partial file; this function never creates or replaces a destination.
/// `None` is the caller's explicit plaintext choice, never an error fallback.
/// # Errors
/// Rejects bad manifest, missing passphrase, changed payload or failed I/O.
pub fn encode(
    input: &mut impl Read,
    output: &mut impl Write,
    manifest: &BackupManifest,
    passphrase: Option<&[u8]>,
    parameters: KdfParameters,
) -> Result<(), BackupError> {
    manifest.validate()?;
    let (mut cipher, encryption) = if let Some(password) = passphrase {
        libsodium_rs::ensure_init().map_err(|_| BackupError::Crypto)?;
        let mut salt = [0; crypto_pwhash::SALTBYTES];
        libsodium_rs::random::fill_bytes(&mut salt);
        let key = key(password, parameters, &salt)?;
        let (cipher, stream_header) =
            stream::PushState::init_push(&key).map_err(|_| BackupError::Crypto)?;
        (
            Some(cipher),
            Some(Encryption {
                kdf: parameters,
                salt,
                stream_header,
            }),
        )
    } else {
        (None, None)
    };
    let header = serde_json::to_vec(&Header {
        major: 1,
        minor: 0,
        encryption,
    })
    .map_err(|_| BackupError::Invalid)?;
    // Authenticate exact bytes including framing, not a parsed/reserialized header.
    let mut associated = MAGIC.to_vec();
    write_record(&mut associated, &header)?;
    output.write_all(&associated)?;
    let mut push = |bytes: &[u8], final_record: bool| -> Result<(), BackupError> {
        if let Some(cipher) = &mut cipher {
            let encrypted = cipher
                .push(
                    bytes,
                    Some(&associated),
                    if final_record {
                        stream::TAG_FINAL
                    } else {
                        stream::TAG_MESSAGE
                    },
                )
                .map_err(|_| BackupError::Crypto)?;
            write_record(output, &encrypted)
        } else {
            write_record(output, bytes)
        }
    };
    let encoded_manifest = serde_json::to_vec(manifest).map_err(|_| BackupError::Manifest)?;
    if encoded_manifest.len() > CHUNK {
        return Err(BackupError::Manifest);
    }
    push(&encoded_manifest, false)?;
    let mut hash = Sha256::new();
    let mut bytes = Zeroizing::new(vec![0; CHUNK]);
    let mut size = 0_u64;
    loop {
        let n = input.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > manifest.database_bytes {
            return Err(BackupError::Invalid);
        }
        hash.update(&bytes[..n]);
        push(&bytes[..n], false)?;
    }
    if size != manifest.database_bytes
        || <[u8; 32]>::from(hash.finalize()) != manifest.database_sha256
    {
        return Err(BackupError::Invalid);
    }
    push(&[], true)?;
    output.flush()?;
    Ok(())
}

fn read_header(input: &mut impl Read) -> Result<(Header, Vec<u8>), BackupError> {
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(BackupError::Invalid);
    }
    let header_bytes = read_record(input, 4096)?;
    let header: Header = serde_json::from_slice(&header_bytes).map_err(|_| BackupError::Invalid)?;
    if header.major != 1 || header.minor != 0 {
        return Err(BackupError::Invalid);
    }
    let mut associated = magic.to_vec();
    write_record(&mut associated, &header_bytes)?;
    if let Some(encryption) = &header.encryption {
        encryption.kdf.validate()?;
    }
    Ok((header, associated))
}

/// Inspect the bounded untrusted header solely to ask for a passphrase.
/// # Errors
/// Rejects unsupported versions, unknown fields and excessive KDF parameters.
pub fn encryption_required(input: &mut impl Read) -> Result<bool, BackupError> {
    Ok(read_header(input)?.0.encryption.is_some())
}

/// Decode only into private staging. The output is untrusted until this returns;
/// even afterwards import must domain-decode it into a newly created database.
/// # Errors
/// Rejects unknown header/manifest fields, excessive KDF/record lengths, bad tags,
/// truncation, reordering, wrong passwords, missing final tag and trailing data.
pub fn decode(
    input: &mut impl Read,
    output: &mut impl Write,
    passphrase: Option<&[u8]>,
) -> Result<BackupManifest, BackupError> {
    let (header, associated) = read_header(input)?;
    let mut cipher = match header.encryption {
        Some(encryption) => {
            encryption.kdf.validate()?;
            let key = key(
                passphrase.ok_or(BackupError::Authentication)?,
                encryption.kdf,
                &encryption.salt,
            )?;
            Some(
                stream::PullState::init_pull(&encryption.stream_header, &key)
                    .map_err(|_| BackupError::Authentication)?,
            )
        }
        None => None,
    };
    let mut pull = |input: &mut dyn Read| -> Result<(Zeroizing<Vec<u8>>, bool), BackupError> {
        let record = read_record(&mut { input }, CHUNK + stream::ABYTES)?;
        if let Some(cipher) = &mut cipher {
            let (plain, tag) = cipher
                .pull(&record, Some(&associated))
                .map_err(|_| BackupError::Authentication)?;
            if !matches!(tag, stream::TAG_MESSAGE | stream::TAG_FINAL) {
                return Err(BackupError::Invalid);
            }
            Ok((Zeroizing::new(plain), tag == stream::TAG_FINAL))
        } else {
            let final_record = record.is_empty();
            Ok((Zeroizing::new(record), final_record))
        }
    };
    let (manifest_bytes, final_record) = pull(input)?;
    if final_record {
        return Err(BackupError::Manifest);
    }
    let manifest: BackupManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| BackupError::Manifest)?;
    manifest.validate()?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    loop {
        let (bytes, final_record) = pull(input)?;
        if final_record {
            if !bytes.is_empty() {
                return Err(BackupError::Invalid);
            }
            break;
        }
        if bytes.is_empty() {
            return Err(BackupError::Invalid);
        }
        size += bytes.len() as u64;
        if size > manifest.database_bytes {
            return Err(BackupError::Invalid);
        }
        hash.update(&bytes);
        output.write_all(&bytes)?;
    }
    let mut trailing = [0];
    if size != manifest.database_bytes
        || <[u8; 32]>::from(hash.finalize()) != manifest.database_sha256
        || input.read(&mut trailing)? != 0
    {
        return Err(BackupError::Invalid);
    }
    output.flush()?;
    Ok(manifest)
}

#[cfg(test)]
#[path = "backup_codec_tests.rs"]
mod tests;
