use super::*;
use agentsassemble_persistence::{AccountAuthority, SqliteStore};
use aws_lc_rs::{
    encoding::{AsDer, Pkcs8V1Der},
    rsa::{KeyPair, KeySize},
    signature::KeyPair as _,
};
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, jwk::JwkSet};
use serde_json::{Value, json};

pub(crate) fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("Google fixture: {error:?}"))
}

pub(crate) async fn service_fixture() -> (GoogleAccountService, EncodingKey) {
    let pair = checked(KeyPair::generate(KeySize::Rsa2048));
    let private_der = checked(AsDer::<Pkcs8V1Der>::as_der(&pair));
    let pem = format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
        STANDARD.encode(private_der.as_ref())
    );
    let key = checked(EncodingKey::from_rsa_pem(pem.as_bytes()));
    let public = pair.public_key();
    let keys: JwkSet = checked(serde_json::from_value(
        json!({"keys": [{"kty": "RSA", "alg": "RS256", "kid": "fixture-key", "use": "sig", "n": URL_SAFE_NO_PAD.encode(public.modulus().big_endian_without_leading_zero()), "e": URL_SAFE_NO_PAD.encode(public.exponent().big_endian_without_leading_zero())}]}),
    ));
    let service = checked(GoogleAccountService::new("fixture-client"));
    checked(service.require_verifier())
        .install_fixture_keys(keys)
        .await;
    (service, key)
}

pub(crate) fn claims(nonce: &str) -> Value {
    let now = chrono::Utc::now().timestamp();
    json!({"iss": "https://accounts.google.com", "aud": "fixture-client", "sub": "fixture-subject", "nonce": nonce, "iat": now, "exp": now + 300})
}

pub(crate) fn token(claims: &Value, key: &EncodingKey) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("fixture-key".into());
    checked(encode(&header, claims, key))
}

#[tokio::test]
async fn google_signature_claims_identity_and_nonce_are_all_required_before_consumption() {
    let (service, key) = service_fixture().await;
    let store = checked(SqliteStore::open("sqlite::memory:").await);
    checked(
        store
            .bootstrap_local_authority("11111111-1111-4111-8111-111111111111", "Host")
            .await,
    );
    let identity = checked(
        store
            .account_identity(AccountAuthority::BrowserDevice([1; 32]))
            .await,
    );
    let wrong_device = checked(
        store
            .account_identity(AccountAuthority::BrowserDevice([2; 32]))
            .await,
    );
    let challenge = checked(service.start(identity.clone()).await);
    let original = claims(&challenge.nonce);
    let valid = token(&original, &key);
    assert!(
        service
            .verify(&wrong_device, &valid, &challenge.nonce)
            .await
            .is_err()
    );
    for (field, value) in [
        ("aud", json!("other-client")),
        ("iss", json!("https://attacker.example")),
        ("nonce", json!("other-nonce")),
        ("exp", json!(1)),
        ("iat", json!(9_999_999_999_u64)),
        ("azp", json!("other-client")),
    ] {
        let mut invalid = original.clone();
        invalid[field] = value;
        assert!(
            service
                .verify(&identity, &token(&invalid, &key), &challenge.nonce)
                .await
                .is_err(),
            "accepted {field}"
        );
    }
    let mut invalid = original.clone();
    checked(invalid.as_object_mut().ok_or("claims")).remove("exp");
    assert!(
        service
            .verify(&identity, &token(&invalid, &key), &challenge.nonce)
            .await
            .is_err()
    );
    let mut corrupted = valid.clone().into_bytes();
    let last = corrupted.len() - 10;
    corrupted[last] = if corrupted[last] == b'A' { b'B' } else { b'A' };
    assert!(
        service
            .verify(
                &identity,
                &checked(String::from_utf8(corrupted)),
                &challenge.nonce
            )
            .await
            .is_err()
    );
    checked(service.verify(&identity, &valid, &challenge.nonce).await);
    assert!(
        service
            .verify(&identity, &valid, &challenge.nonce)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn challenge_capacity_replacement_expiry_and_disabled_configuration_are_explicit() {
    let service = checked(GoogleAccountService::new("fixture-client"));
    let store = checked(SqliteStore::open("sqlite::memory:").await);
    checked(
        store
            .bootstrap_local_authority("11111111-1111-4111-8111-111111111111", "Host")
            .await,
    );
    let identity = checked(
        store
            .account_identity(AccountAuthority::BrowserDevice([0; 32]))
            .await,
    );
    assert!(!GoogleAccountService::default().configuration().enabled);
    assert!(
        GoogleAccountService::default()
            .start(identity.clone())
            .await
            .is_err()
    );
    let mut identities = Vec::new();
    for n in 1..=MAX_UNBOUND_CHALLENGES {
        identities.push(checked(
            store
                .account_identity(AccountAuthority::BrowserDevice(
                    [checked(u8::try_from(n)); 32],
                ))
                .await,
        ));
    }
    tokio::time::pause();
    let first = checked(service.start(identity.clone()).await);
    let replacement = checked(service.start(identity.clone()).await);
    assert!(
        service
            .validate_challenge(&identity, &first.nonce, false)
            .await
            .is_err()
    );
    for next in &identities[..MAX_UNBOUND_CHALLENGES - 1] {
        checked(service.start(next.clone()).await);
    }
    let extra = identities[MAX_UNBOUND_CHALLENGES - 1].clone();
    assert_eq!(
        checked(service.start(extra).await.err().ok_or("capacity rejection")).code,
        "google_login_capacity_exceeded"
    );
    tokio::time::advance(CHALLENGE_TTL).await;
    assert!(
        service
            .validate_challenge(&identity, &replacement.nonce, false)
            .await
            .is_err()
    );
    checked(service.start(identity).await);
}
