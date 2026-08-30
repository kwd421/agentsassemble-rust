use std::{path::Path, time::Duration};

pub(super) const FIXTURE_READINESS_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) async fn wait_for_path(path: &Path) {
    tokio::time::timeout(FIXTURE_READINESS_TIMEOUT, async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("provider fixture did not publish its request marker"));
}
