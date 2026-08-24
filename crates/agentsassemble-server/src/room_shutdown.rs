use std::time::Duration;

use tokio::{task::JoinHandle, time::Instant};

use crate::RoomShutdownError;

pub(super) async fn join_room_tasks(
    tasks: Vec<JoinHandle<()>>,
    timeout: Duration,
) -> Result<(), RoomShutdownError> {
    let deadline = Instant::now() + timeout;
    let mut failure = None;
    for mut task in tasks {
        match tokio::time::timeout_at(deadline, &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                failure.get_or_insert_with(|| RoomShutdownError::TaskFailed(error.to_string()));
            }
            Err(_) => {
                task.abort();
                let _ = task.await;
                failure.get_or_insert(RoomShutdownError::TimedOut);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::RoomShutdownError;

    #[tokio::test]
    async fn stalled_room_task_is_aborted_within_one_deadline() {
        let task = tokio::spawn(std::future::pending::<()>());
        let result = super::join_room_tasks(vec![task], Duration::from_millis(10)).await;
        assert_eq!(result, Err(RoomShutdownError::TimedOut));
    }
}
