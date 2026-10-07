use super::*;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

// Real WebCrypto peer, not a second Rust implementation. Any transcript/label/
// nonce/AAD mismatch fails at signature verification or authenticated decryption.
#[tokio::test]
async fn webcrypto_ring_bidirectional_cipher_and_replay_rejection()
-> Result<(), Box<dyn std::error::Error>> {
    let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:").await?;
    let identity =
        super::super::CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let mut peer = tokio::process::Command::new("node")
        .arg("-e")
        .arg(include_str!("secure_crypto_peer.mjs"))
        .arg("--input-type=module")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = peer.stdin.take().ok_or("no stdin")?;
    let mut output = BufReader::new(peer.stdout.take().ok_or("no stdout")?).lines();
    input
        .write_all(
            format!(
                "{}\n",
                json!({"server_id":identity.server_id(),"key":identity.public_key_x()})
            )
            .as_bytes(),
        )
        .await?;
    let client = serde_json::from_str(&output.next_line().await?.ok_or("no hello")?)?;
    let mut agreement = identity.secure_handshake(client)?;
    input
        .write_all(format!("{}\n", serde_json::to_string(&agreement.hello)?).as_bytes())
        .await?;
    let record: String = serde_json::from_str(&output.next_line().await?.ok_or("no record")?)?;
    let record = URL_SAFE_NO_PAD.decode(record)?;
    let mut tampered = record.clone();
    tampered[8] ^= 1;
    assert!(agreement.receive.open(&tampered).is_err());
    assert_eq!(agreement.receive.open(&record)?, b"client possession");
    assert!(agreement.receive.open(&record).is_err());
    let response = agreement.send.seal(b"host response")?;
    assert!(agreement.receive.open(&response).is_err());
    input
        .write_all(format!("{}\n", json!(URL_SAFE_NO_PAD.encode(response))).as_bytes())
        .await?;
    assert_eq!(
        output.next_line().await?.ok_or("no response")?,
        "\"host response\""
    );
    assert!(peer.wait().await?.success());
    Ok(())
}
