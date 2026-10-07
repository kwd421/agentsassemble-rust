//! Host-key authenticated P-256 agreement and ordered directional AEAD records.
//! `WebCrypto` uses these exact transcript/AAD bytes; no product authorization lives here.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    aead, agreement, hkdf,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SECURE_PROTOCOL: &str = "secure_admission_v1";
pub const MAX_SECURE_PLAINTEXT: usize = 96 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecureClientHello {
    pub protocol: String,
    pub server_id: String,
    pub registration_epoch: String,
    pub origin: String,
    pub generation: i64,
    pub purpose: String,
    pub client_nonce: String,
    pub client_public_key: String,
}

#[derive(Serialize)]
pub struct SecureServerHello {
    #[serde(flatten)]
    pub client: SecureClientHello,
    pub host_ephemeral_public_key: String,
    pub channel_id: String,
    pub issued_at: i64,
    pub signature: String,
}

impl SecureServerHello {
    fn transcript(&self) -> Vec<u8> {
        json!([
            "AA-SECURE-ADMISSION-1",
            self.client.protocol,
            self.client.server_id,
            self.client.registration_epoch,
            self.client.origin,
            self.client.generation,
            self.client.purpose,
            self.client.client_nonce,
            self.client.client_public_key,
            self.host_ephemeral_public_key,
            self.channel_id,
            self.issued_at
        ])
        .to_string()
        .into_bytes()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Secure channel authentication failed")]
pub struct SecureChannelError;

pub struct SecureHandshake {
    pub hello: SecureServerHello,
    pub receive: SecureCipher,
    pub send: SecureCipher,
}

pub struct SecureCipher {
    key: aead::LessSafeKey,
    channel: String,
    direction: &'static str,
    next: u64,
}

impl SecureHandshake {
    pub(super) fn new(
        client: SecureClientHello,
        sign: impl FnOnce(&[u8]) -> String,
    ) -> Result<Self, SecureChannelError> {
        if client.protocol != SECURE_PROTOCOL
            || client.registration_epoch.is_empty()
            || client.registration_epoch.len() > 128
            || client.generation < 1
            || !matches!(
                client.purpose.as_str(),
                "owner" | "member_admission" | "member_connect"
            )
        {
            return Err(SecureChannelError);
        }
        let nonce = decode_fixed(&client.client_nonce, 32)?;
        let public = decode_fixed(&client.client_public_key, 65)?;
        let random = SystemRandom::new();
        let private = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &random)
            .map_err(|_| SecureChannelError)?;
        let host_public = private
            .compute_public_key()
            .map_err(|_| SecureChannelError)?;
        let mut channel = [0; 32];
        random.fill(&mut channel).map_err(|_| SecureChannelError)?;
        let mut hello = SecureServerHello {
            client,
            host_ephemeral_public_key: URL_SAFE_NO_PAD.encode(host_public.as_ref()),
            channel_id: URL_SAFE_NO_PAD.encode(channel),
            issued_at: chrono::Utc::now().timestamp(),
            signature: String::new(),
        };
        let transcript = hello.transcript();
        hello.signature = sign(&transcript);
        let peer = agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, public);
        let (receive, send) = agreement::agree_ephemeral(private, &peer, |secret| {
            let extracted = hkdf::Salt::new(hkdf::HKDF_SHA256, &nonce).extract(secret);
            Ok((
                SecureCipher::derive(&extracted, &transcript, &hello.channel_id, "c2h")?,
                SecureCipher::derive(&extracted, &transcript, &hello.channel_id, "h2c")?,
            ))
        })
        .map_err(|_| SecureChannelError)??;
        Ok(Self {
            hello,
            receive,
            send,
        })
    }
}

impl SecureCipher {
    fn derive(
        prk: &hkdf::Prk,
        transcript: &[u8],
        channel: &str,
        direction: &'static str,
    ) -> Result<Self, SecureChannelError> {
        let info = [transcript, direction.as_bytes()];
        let expanded = prk
            .expand(&info, &aead::AES_256_GCM)
            .map_err(|_| SecureChannelError)?;
        let key = aead::UnboundKey::from(expanded);
        Ok(Self {
            key: aead::LessSafeKey::new(key),
            channel: channel.into(),
            direction,
            next: 0,
        })
    }

    fn nonce(&self) -> aead::Nonce {
        let mut nonce = [0; 12];
        nonce[4..].copy_from_slice(&self.next.to_be_bytes());
        aead::Nonce::assume_unique_for_key(nonce)
    }

    fn aad(&self) -> aead::Aad<Vec<u8>> {
        aead::Aad::from(
            json!([
                SECURE_PROTOCOL,
                self.channel,
                self.direction,
                self.next.to_string()
            ])
            .to_string()
            .into_bytes(),
        )
    }

    /// # Errors
    /// Rejects oversized records or counter exhaustion. The owner closes on any error.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, SecureChannelError> {
        if plaintext.len() > MAX_SECURE_PLAINTEXT || self.next == u64::MAX {
            return Err(SecureChannelError);
        }
        let mut ciphertext = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(self.nonce(), self.aad(), &mut ciphertext)
            .map_err(|_| SecureChannelError)?;
        let mut record = self.next.to_be_bytes().to_vec();
        record.extend(ciphertext);
        self.next += 1;
        Ok(record)
    }

    /// # Errors
    /// Rejects tampering, reflection, replay, gaps and oversized records before dispatch.
    pub fn open(&mut self, record: &[u8]) -> Result<Vec<u8>, SecureChannelError> {
        if record.len() < 24
            || record.len() > MAX_SECURE_PLAINTEXT + 24
            || self.next == u64::MAX
            || record[..8] != self.next.to_be_bytes()
        {
            return Err(SecureChannelError);
        }
        let mut ciphertext = record[8..].to_vec();
        let plaintext = self
            .key
            .open_in_place(self.nonce(), self.aad(), &mut ciphertext)
            .map_err(|_| SecureChannelError)?
            .to_vec();
        self.next += 1;
        Ok(plaintext)
    }
}

pub(crate) fn decode_fixed(encoded: &str, length: usize) -> Result<Vec<u8>, SecureChannelError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| SecureChannelError)?;
    if bytes.len() != length || URL_SAFE_NO_PAD.encode(&bytes) != encoded {
        return Err(SecureChannelError);
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "secure_handshake_tests.rs"]
mod tests;
