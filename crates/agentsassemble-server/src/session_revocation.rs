/// A committed revocation, with the one response still authorized by a self-leave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRevocation {
    pub(crate) fingerprint: [u8; 32],
    pub(crate) final_leave_request: Option<String>,
}

impl From<[u8; 32]> for SessionRevocation {
    fn from(fingerprint: [u8; 32]) -> Self {
        Self {
            fingerprint,
            final_leave_request: None,
        }
    }
}
