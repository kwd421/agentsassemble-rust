use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
use tauri_plugin_updater::UpdaterExt;

const PAYLOAD: &[u8] = b"AgentsAssemble updater signature fixture\n";

#[test]
fn production_policy_rejects_unsigned_versions_and_insecure_endpoints() {
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .unwrap_or_else(|e| panic!("config: {e}"));
    let policy: tauri_plugin_updater::Config =
        serde_json::from_value(config["plugins"]["updater"].clone())
            .unwrap_or_else(|e| panic!("updater policy: {e}"));
    assert!(policy.require_signed_version);
    assert!(!policy.allow_downgrades);
    assert!(!policy.dangerous_insecure_transport_protocol);
    assert!(!policy.dangerous_accept_invalid_certs);
    assert!(!policy.dangerous_accept_invalid_hostnames);
    assert!(policy.endpoints.iter().all(|url| url.scheme() == "https"));
}

#[test]
fn official_updater_verifies_bytes_and_the_announced_version()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("update_signature_fixture.json"))?;
    let production: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))?;
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    let mut config = production["plugins"]["updater"].clone();
    config["pubkey"] = fixture["public_key"].clone();
    // Only the in-process fixture uses HTTP; the production policy above forbids it.
    config["dangerousInsecureTransportProtocol"] = true.into();
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".into(), config);
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)?;
    for (version, payload, valid) in [
        ("9.0.0", PAYLOAD, true),
        ("9.0.0", b"tampered executable".as_slice(), false),
        ("10.0.0", PAYLOAD, false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let base = format!("http://{}", listener.local_addr()?);
        let manifest = serde_json::to_vec(&serde_json::json!({
            "version": version, "url": format!("{base}/artifact"),
            "signature": fixture["signature"]
        }))?;
        let server = std::thread::spawn(move || -> std::io::Result<()> {
            for body in [&manifest[..], payload] {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut request = [0; 4096];
                let _ = stream.read(&mut request)?;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                    body.len()
                )?;
                stream.write_all(body)?;
            }
            Ok(())
        });
        let updater = app
            .updater_builder()
            .endpoints(vec![base.parse()?])?
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .build()?;
        let result = tauri::async_runtime::block_on(async {
            let update = updater.check().await?.ok_or("expected update")?;
            let result = update.download(|_, _| {}, || {}).await;
            Ok::<_, Box<dyn std::error::Error>>(result)
        })?;
        server.join().map_err(|_| "fixture server panicked")??;
        assert_eq!(result.is_ok(), valid, "version={version}");
        if valid {
            assert_eq!(result?, PAYLOAD);
        }
    }
    Ok(())
}
