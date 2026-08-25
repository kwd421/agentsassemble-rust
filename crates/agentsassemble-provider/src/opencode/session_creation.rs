use std::future::Future;

use crate::{opencode_protocol::session_unconfirmed, runtime::DriverError};

#[derive(Default)]
pub(super) struct SessionCreationAuthority {
    uncertain: bool,
}

impl SessionCreationAuthority {
    fn begin(&mut self) -> Result<(), DriverError> {
        if self.uncertain {
            return Err(session_unconfirmed());
        }
        self.uncertain = true;
        Ok(())
    }

    fn confirm(&mut self) {
        self.uncertain = false;
    }

    pub(super) const fn replay_is_safe(&self) -> bool {
        !self.uncertain
    }
}

pub(super) async fn guarded_session_creation<T, F>(
    authority: &mut SessionCreationAuthority,
    request: F,
) -> Result<T, DriverError>
where
    F: Future<Output = Result<T, DriverError>>,
{
    authority.begin()?;
    let result = request.await?;
    authority.confirm();
    Ok(result)
}
