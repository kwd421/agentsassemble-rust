use std::{sync::Arc, time::Duration};

use tokio::sync::{Mutex, Notify};

use super::{DriverError, ProviderDriver};

pub(super) struct DriverCell {
    driver: Mutex<Option<Box<dyn ProviderDriver>>>,
    available: Notify,
}

impl DriverCell {
    pub(super) fn new(driver: Box<dyn ProviderDriver>) -> Arc<Self> {
        Arc::new(Self {
            driver: Mutex::new(Some(driver)),
            available: Notify::new(),
        })
    }

    pub(super) async fn take(&self) -> Result<Box<dyn ProviderDriver>, DriverError> {
        loop {
            let notified = self.available.notified();
            if let Some(driver) = self.driver.lock().await.take() {
                return Ok(driver);
            }
            notified.await;
        }
    }

    pub(super) fn try_take(&self) -> Result<Box<dyn ProviderDriver>, DriverError> {
        self.driver
            .try_lock()
            .map_err(|_| operation_in_progress())?
            .take()
            .ok_or_else(operation_in_progress)
    }

    pub(super) async fn put(&self, driver: Box<dyn ProviderDriver>) {
        let mut slot = self.driver.lock().await;
        debug_assert!(slot.is_none(), "driver cell must have one exclusive owner");
        *slot = Some(driver);
        drop(slot);
        self.available.notify_waiters();
    }

    pub(super) async fn wait_take(
        &self,
        timeout: Duration,
    ) -> Result<Box<dyn ProviderDriver>, DriverError> {
        tokio::time::timeout(timeout, async {
            loop {
                let notified = self.available.notified();
                if let Some(driver) = self.driver.lock().await.take() {
                    return driver;
                }
                notified.await;
            }
        })
        .await
        .map_err(|_| {
            DriverError::new(
                "provider_turn_quiescence_timeout",
                "The active provider turn did not return runtime ownership in time.",
            )
        })
    }
}

const fn operation_in_progress() -> DriverError {
    DriverError::new(
        "operation_in_progress",
        "The provider driver is owned by another exact operation.",
    )
}
