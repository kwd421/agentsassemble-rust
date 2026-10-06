//! Runs within the single host directory sender and its cancellation lifecycle.
use super::{
    CentralDirectoryError, CentralDirectoryInner, CentralHostIdentity, Method, SqliteStore, Utc,
    json, send_signed,
};

pub(super) async fn send(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    identity: &CentralHostIdentity,
) -> Result<(), CentralDirectoryError> {
    let now = Utc::now().timestamp();
    let batch = store
        .take_member_projection_batch(now, uuid::Uuid::new_v4().as_bytes()[0])
        .await?;
    let Some(first) = batch.first() else {
        return Ok(());
    };
    let path = format!("/v1/servers/{}/member-results", identity.server_id());
    let result = send_signed(
        inner,
        identity,
        store,
        Method::POST,
        &path,
        json!({"registration_epoch": first.registration_epoch, "results": batch}),
    )
    .await;
    match result {
        Ok(bytes) => {
            let response: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| CentralDirectoryError::InvalidResponse)?;
            let results = response["results"]
                .as_array()
                .ok_or(CentralDirectoryError::InvalidResponse)?;
            if results.len() != batch.len() {
                return Err(CentralDirectoryError::InvalidResponse);
            }
            for item in &batch {
                let ack = results
                    .iter()
                    .find(|r| {
                        r["projection_id"].as_str() == Some(&item.projection_id)
                            && r["revision"].as_i64() == Some(item.revision)
                    })
                    .ok_or(CentralDirectoryError::InvalidResponse)?;
                let permanent = match ack["status"].as_str() {
                    Some("applied" | "stale") => false,
                    Some("conflict") => true,
                    _ => return Err(CentralDirectoryError::InvalidResponse),
                };
                store
                    .finish_member_projection_batch(std::slice::from_ref(item), permanent, now)
                    .await?;
            }
            Ok(())
        }
        Err(CentralDirectoryError::Rejected) => {
            store
                .finish_member_projection_batch(&batch, true, now)
                .await?;
            Err(CentralDirectoryError::Rejected)
        }
        Err(error) => Err(error), // 429, transport, 5xx keep the durable backoff reservation.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::central::directory::{CancellationToken, CentralDirectory};
    #[tokio::test]
    async fn report_keeps_immutable_epoch_and_budget_429_is_retryable()
    -> Result<(), Box<dyn std::error::Error>> {
        use axum::{Router, body::Bytes, routing::post};
        let store = SqliteStore::open("sqlite::memory:").await?;
        store.set_registration_epoch(Some("new-epoch")).await?;
        let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = Router::new().route(
            "/v1/servers/{id}/member-results",
            post(|body: Bytes| async move {
                let value: serde_json::Value =
                    serde_json::from_slice(&body).unwrap_or_else(|_| panic!("body"));
                assert_eq!(value["registration_epoch"], "old-epoch");
                assert!(value["results"][0].get("person_id").is_none());
                (
                    axum::http::StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(json!({"error":{"code":"temporary_capacity_exhausted"}})),
                )
            }),
        );
        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        let directory = CentralDirectory::configured(&format!("http://{address}"))?;
        let inner = directory.0.as_ref().ok_or("configured")?;
        let response=send_signed(inner,&identity,&store,Method::POST,&format!("/v1/servers/{}/member-results",identity.server_id()),json!({"registration_epoch":"old-epoch","results":[{"projection_id":"AAAAAAAAAAAAAAAAAAAAAA","state":"active","revision":1}]})).await;
        assert!(matches!(
            response,
            Err(CentralDirectoryError::CapacityExhausted)
        ));
        cancel.cancel();
        task.await??;
        Ok(())
    }
}
