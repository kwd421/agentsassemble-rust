use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::super::{AcpClient, AcpClientConfiguration, MAX_PROTOCOL_LINE_BYTES};

#[tokio::test]
async fn typed_configuration_requires_a_complete_noncontradictory_receipt() {
    for fault in ["none", "reset", "duplicate", "missing"] {
        let (input, peer_input) = tokio::io::duplex(MAX_PROTOCOL_LINE_BYTES);
        let (mut peer_output, output) = tokio::io::duplex(MAX_PROTOCOL_LINE_BYTES);
        let peer = tokio::spawn(async move {
            let mut lines = BufReader::new(peer_input).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let message: Value =
                    serde_json::from_str(&line).unwrap_or_else(|error| panic!("request: {error}"));
                let result = match message["method"].as_str() {
                    Some("initialize") => {
                        assert_eq!(
                            message["params"]["clientCapabilities"]["_meta"]["parameterizedModelPicker"],
                            true
                        );
                        json!({"protocolVersion":1,"agentCapabilities":{}})
                    }
                    Some("cursor/list_available_models") => json!({"models":[]}),
                    Some("session/set_config_option") => {
                        assert_eq!(message["params"]["configId"], "fast");
                        assert_eq!(message["params"]["value"], "true");
                        let mut options =
                            options(if fault == "reset" { "other" } else { "default" }, "true");
                        if fault == "duplicate" {
                            options.push(options[0].clone());
                        }
                        if fault == "missing" {
                            options.remove(0);
                        }
                        json!({"configOptions":options})
                    }
                    other => panic!("unexpected method: {other:?}"),
                };
                let bytes = format!(
                    "{}\n",
                    json!({"jsonrpc":"2.0","id":message["id"],"result":result})
                );
                peer_output
                    .write_all(bytes.as_bytes())
                    .await
                    .unwrap_or_else(|error| panic!("reply: {error}"));
            }
        });
        let mut configuration = AcpClientConfiguration::default();
        configuration.capabilities.meta = Some(serde_json::Map::from_iter([(
            "parameterizedModelPicker".into(),
            json!(true),
        )]));
        let mut client = AcpClient::connect(input, output, configuration)
            .await
            .unwrap_or_else(|error| panic!("connect: {:?}", error.error));
        assert_eq!(
            client
                .request_extension("cursor/list_available_models", json!({}))
                .await
                .unwrap_or_else(|error| panic!("extension: {error}")),
            json!({"models":[]})
        );
        let options = serde_json::from_value(json!(options("default", "false")))
            .unwrap_or_else(|error| panic!("options: {error}"));
        let result = client
            .select_configuration(
                &"session".into(),
                Some(options),
                &[
                    ("model".into(), "default".into()),
                    ("fast".into(), "true".into()),
                ],
            )
            .await;
        assert_eq!(result.is_ok(), fault == "none", "receipt {fault}");
        assert_eq!(client.requires_restart(), fault != "none");
        client.shutdown().await;
        peer.await
            .unwrap_or_else(|error| panic!("fixture: {error}"));
    }
}

fn options(model: &str, fast: &str) -> Vec<Value> {
    vec![
        json!({"id":"model","name":"Model","type":"select","currentValue":model,"options":[{"value":"default","name":"Auto"},{"value":"other","name":"Other"}]}),
        json!({"id":"fast","name":"Fast","type":"select","currentValue":fast,"options":[{"value":"false","name":"Off"},{"value":"true","name":"On"}]}),
    ]
}
