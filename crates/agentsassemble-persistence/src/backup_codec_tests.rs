use super::*;
use std::io::Cursor;

fn fixture() -> (Vec<u8>, BackupManifest) {
    let data = vec![42; CHUNK + 127];
    let manifest = BackupManifest {
        schema_version: crate::schema_version::CURRENT_SCHEMA_VERSION,
        server_id: uuid::Uuid::new_v4().to_string(),
        server_name: "서버".into(),
        issuer: "https://central.example".into(),
        owner_person_id: "owner-person".into(),
        source_epoch: "source-epoch".into(),
        source_key_fingerprint: "f".repeat(43),
        snapshot_at: chrono::Utc::now(),
        rooms: 2,
        messages: 10,
        assets: 1,
        database_bytes: data.len() as u64,
        database_sha256: Sha256::digest(&data).into(),
    };
    (data, manifest)
}
fn parameters() -> KdfParameters {
    KdfParameters {
        operations: 2,
        memory_bytes: 64 * 1024 * 1024,
    }
}

// Boundary: exported bytes reconstruct exactly; wrong secrets/corrupted framing
// never return a validated manifest. Removing final-tag/hash/EOF checks fails.
#[test]
fn authenticated_and_explicit_plaintext_roundtrip_reject_corruption() -> Result<(), BackupError> {
    let (data, manifest) = fixture();
    for password in [None, Some(b"test backup secret".as_slice())] {
        let mut encoded = Vec::new();
        encode(
            &mut data.as_slice(),
            &mut encoded,
            &manifest,
            password,
            parameters(),
        )?;
        let mut recovered = Vec::new();
        let decoded = decode(&mut encoded.as_slice(), &mut recovered, password)?;
        assert_eq!(recovered, data);
        assert_eq!(decoded.server_id, manifest.server_id);
        for end in [0, 8, 11, encoded.len() / 2, encoded.len() - 1] {
            assert!(decode(&mut &encoded[..end], &mut std::io::sink(), password).is_err());
        }
        let mut flipped = encoded.clone();
        let last_data = flipped.len() - 30;
        flipped[last_data] ^= 1;
        assert!(decode(&mut flipped.as_slice(), &mut std::io::sink(), password).is_err());
        encoded.push(0);
        assert!(decode(&mut encoded.as_slice(), &mut std::io::sink(), password).is_err());
    }
    Ok(())
}

#[test]
fn encrypted_records_cannot_be_reordered_or_opened_with_another_password() -> Result<(), BackupError>
{
    let (data, manifest) = fixture();
    let mut encoded = Vec::new();
    encode(
        &mut data.as_slice(),
        &mut encoded,
        &manifest,
        Some(b"original"),
        parameters(),
    )?;
    assert!(
        decode(
            &mut encoded.as_slice(),
            &mut std::io::sink(),
            Some(b"wrong")
        )
        .is_err()
    );
    assert!(decode(&mut encoded.as_slice(), &mut std::io::sink(), None).is_err());
    let mut input = Cursor::new(encoded);
    input.set_position(8);
    let header = read_record(&mut input, 4096)?;
    let manifest = read_record(&mut input, CHUNK + stream::ABYTES)?;
    let a = read_record(&mut input, CHUNK + stream::ABYTES)?;
    let b = read_record(&mut input, CHUNK + stream::ABYTES)?;
    let final_record = read_record(&mut input, CHUNK + stream::ABYTES)?;
    let mut reordered = MAGIC.to_vec();
    for bytes in [&header, &manifest, &b, &a, &final_record] {
        write_record(&mut reordered, bytes)?;
    }
    assert!(
        decode(
            &mut reordered.as_slice(),
            &mut std::io::sink(),
            Some(b"original")
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn hostile_kdf_and_unknown_fields_fail_before_payload_or_large_allocation()
-> Result<(), BackupError> {
    let bad_headers = [
        serde_json::json!({"major":1,"minor":0,"encryption":null,"unknown":true}),
        serde_json::json!({"major":1,"minor":0,"encryption":{
            "kdf":{"operations":u64::MAX,"memory_bytes":usize::MAX},
            "salt":vec![0;16],"stream_header":vec![0;24]}}),
    ];
    for header in bad_headers {
        let mut encoded = MAGIC.to_vec();
        write_record(
            &mut encoded,
            &serde_json::to_vec(&header).map_err(|_| BackupError::Invalid)?,
        )?;
        assert!(matches!(
            decode(&mut encoded.as_slice(), &mut std::io::sink(), Some(b"pass")),
            Err(BackupError::Invalid)
        ));
    }
    let mut oversized = MAGIC.to_vec();
    oversized.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        decode(&mut oversized.as_slice(), &mut std::io::sink(), None),
        Err(BackupError::Invalid)
    ));
    Ok(())
}

#[test]
#[ignore = "explicit local KDF measurement; no network or provider"]
fn measure_kdf() -> Result<(), BackupError> {
    for parameters in [
        parameters(),
        KdfParameters {
            operations: 3,
            memory_bytes: 256 * 1024 * 1024,
        },
        KdfParameters {
            operations: 4,
            memory_bytes: 256 * 1024 * 1024,
        },
    ] {
        let mut samples = Vec::new();
        for _ in 0..5 {
            let started = std::time::Instant::now();
            let _derived = key(b"local synthetic measurement", parameters, &[7; 16])?;
            samples.push(started.elapsed().as_millis());
        }
        println!(
            "Argon2id operations={} memory_mib={} elapsed_ms={samples:?}",
            parameters.operations,
            parameters.memory_bytes / (1024 * 1024)
        );
    }
    Ok(())
}

#[test]
fn plaintext_replacement_cannot_bypass_supplied_passphrase() -> Result<(), BackupError> {
    let (data, manifest) = fixture();
    let mut encoded = Vec::new();
    encode(
        &mut data.as_slice(),
        &mut encoded,
        &manifest,
        None,
        parameters(),
    )?;
    let mut output = Vec::new();
    assert!(matches!(
        decode(
            &mut encoded.as_slice(),
            &mut output,
            Some(b"selected encrypted backup")
        ),
        Err(BackupError::Authentication)
    ));
    assert!(output.is_empty());
    Ok(())
}

// A rejected capacity check must consume only the manifest, never payload bytes.
#[test]
fn capacity_preflight_precedes_payload_on_the_same_stream() -> Result<(), BackupError> {
    let (data, manifest) = fixture();
    for password in [None, Some(b"secret".as_slice())] {
        let mut encoded = Vec::new();
        encode(
            &mut data.as_slice(),
            &mut encoded,
            &manifest,
            password,
            parameters(),
        )?;
        let mut input = Cursor::new(encoded);
        let mut output = Vec::new();
        let result = decode_with_preflight(&mut input, &mut output, password, |seen| {
            assert_eq!(seen.database_bytes, data.len() as u64);
            assert_eq!(seen.database_sha256, manifest.database_sha256);
            Err(BackupError::Io(std::io::ErrorKind::StorageFull.into()))
        });
        assert!(
            matches!(result, Err(BackupError::Io(error)) if error.kind() == std::io::ErrorKind::StorageFull)
        );
        assert!(output.is_empty());
        assert!(input.position() < input.get_ref().len() as u64);
    }
    Ok(())
}

#[test]
fn unsupported_manifest_versions_fail_before_payload_output() -> Result<(), BackupError> {
    let (data, manifest) = fixture();
    let mut encoded = Vec::new();
    encode(
        &mut data.as_slice(),
        &mut encoded,
        &manifest,
        None,
        parameters(),
    )?;
    let mut input = Cursor::new(encoded);
    input.set_position(8);
    let header = read_record(&mut input, 4096)?;
    let original = read_record(&mut input, CHUNK)?;
    let mut remainder = Vec::new();
    input.read_to_end(&mut remainder)?;
    for version in [
        serde_json::json!(86),
        serde_json::json!(crate::schema_version::CURRENT_SCHEMA_VERSION + 1),
        serde_json::json!("87"),
        serde_json::json!(87.5),
    ] {
        let mut value: serde_json::Value =
            serde_json::from_slice(&original).map_err(|_| BackupError::Manifest)?;
        value["schema_version"] = version;
        let mut altered = MAGIC.to_vec();
        write_record(&mut altered, &header)?;
        write_record(
            &mut altered,
            &serde_json::to_vec(&value).map_err(|_| BackupError::Manifest)?,
        )?;
        altered.extend_from_slice(&remainder);
        let mut output = Vec::new();
        assert!(matches!(
            decode_with_preflight(&mut altered.as_slice(), &mut output, None, |_| panic!(
                "unsupported manifest reached preflight"
            )),
            Err(BackupError::Manifest)
        ));
        assert!(output.is_empty());
    }
    Ok(())
}
