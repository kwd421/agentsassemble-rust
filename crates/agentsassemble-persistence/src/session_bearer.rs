use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

pub const GUEST_RECOVERY_CODE_PREFIX: &str = "aagr1.";
pub const HUMAN_SESSION_BEARER_PREFIX: &str = "aas1.";
pub const HUMAN_SESSION_BEARER_BYTES: usize = 32;
pub const HUMAN_SESSION_BEARER_CHARS: usize =
    HUMAN_SESSION_BEARER_PREFIX.len() + (HUMAN_SESSION_BEARER_BYTES * 4).div_ceil(3);
pub const OPERATOR_SESSION_BEARER_PREFIX: &str = "aops1.";
pub const OPERATOR_SESSION_BEARER_CHARS: usize =
    OPERATOR_SESSION_BEARER_PREFIX.len() + (32_usize * 4).div_ceil(3);

#[derive(Clone, Copy)]
pub(crate) enum SessionBearerPurpose {
    HumanAdmission,
    OperatorPairing,
    GuestIdentityRecovery,
}

pub(crate) struct IssuedBearer {
    pub(crate) bearer: String,
    pub(crate) fingerprint: [u8; 32],
}

pub(crate) fn derive_session_bearer(
    key: &[u8; 32],
    seed: &[u8; 32],
    purpose: SessionBearerPurpose,
) -> IssuedBearer {
    let (context, prefix): (&[u8], &str) = match purpose {
        SessionBearerPurpose::HumanAdmission => (
            b"agentsassemble-human-session-bearer-v1\0",
            HUMAN_SESSION_BEARER_PREFIX,
        ),
        SessionBearerPurpose::GuestIdentityRecovery => {
            (b"agentsassemble-guest-recovery-code-v1\0", "aagr1.")
        }
        SessionBearerPurpose::OperatorPairing => (
            b"agentsassemble-operator-session-bearer-v1\0",
            OPERATOR_SESSION_BEARER_PREFIX,
        ),
    };
    let mut signer = Hmac::<Sha256>::new_from_slice(key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts a 32-byte key"));
    signer.update(context);
    signer.update(seed);
    let mac: [u8; 32] = signer.finalize().into_bytes().into();
    let mut bearer = String::with_capacity(prefix.len() + (32_usize * 4).div_ceil(3));
    bearer.push_str(prefix);
    URL_SAFE_NO_PAD.encode_string(mac, &mut bearer);
    let fingerprint = Sha256::digest(bearer.as_bytes()).into();
    IssuedBearer {
        bearer,
        fingerprint,
    }
}
