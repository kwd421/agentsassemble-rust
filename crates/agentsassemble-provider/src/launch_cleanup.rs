use crate::{driver::DriverError, launch_error::DriverLaunchError, room_portal::RoomPortal};

pub(crate) async fn portal(
    portal: &mut RoomPortal,
    failure: DriverLaunchError,
) -> DriverLaunchError {
    match portal.shutdown().await {
        Ok(()) => failure,
        Err(_) => DriverLaunchError::uncertain(unconfirmed()),
    }
}

pub(crate) async fn owned_and_portal(
    portal: &mut RoomPortal,
    owned: Result<(), DriverError>,
    failure: DriverLaunchError,
) -> DriverLaunchError {
    let portal = portal.shutdown().await;
    match (owned, portal) {
        (Err(error), _) => DriverLaunchError::uncertain(error),
        (Ok(()), Err(_)) => DriverLaunchError::uncertain(unconfirmed()),
        (Ok(()), Ok(())) => failure,
    }
}

pub(crate) const fn unconfirmed() -> DriverError {
    DriverError::new(
        "provider_launch_cleanup_unconfirmed",
        "Provider runtime cleanup could not be confirmed after launch failure.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAUNCH_FAILURE: DriverError =
        DriverError::new("fixture_launch_failed", "The fixture launch failed.");
    const PROCESS_FAILURE: DriverError = DriverError::new(
        "fixture_process_cleanup_failed",
        "The fixture process cleanup failed.",
    );

    #[tokio::test]
    async fn confirmed_portal_cleanup_preserves_the_launch_result() {
        let mut room_portal = RoomPortal::create()
            .await
            .unwrap_or_else(|error| panic!("create launch cleanup portal: {error}"));
        let failure = DriverLaunchError::safe(LAUNCH_FAILURE);

        assert_eq!(portal(&mut room_portal, failure.clone()).await, failure);
        assert!(!room_portal.is_running());
    }

    #[tokio::test]
    async fn failed_owned_cleanup_remains_uncertain_after_portal_shutdown() {
        let mut room_portal = RoomPortal::create()
            .await
            .unwrap_or_else(|error| panic!("create owned cleanup portal: {error}"));
        let result = owned_and_portal(
            &mut room_portal,
            Err(PROCESS_FAILURE),
            DriverLaunchError::safe(LAUNCH_FAILURE),
        )
        .await;

        assert_eq!(result, DriverLaunchError::uncertain(PROCESS_FAILURE));
        assert!(!room_portal.is_running());
    }
}
