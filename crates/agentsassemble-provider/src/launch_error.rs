use crate::runtime::DriverError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DriverLaunchError {
    pub(crate) error: DriverError,
    pub(crate) effect_uncertain: bool,
}

impl DriverLaunchError {
    pub(crate) const fn safe(error: DriverError) -> Self {
        Self {
            error,
            effect_uncertain: false,
        }
    }

    pub(crate) const fn uncertain(error: DriverError) -> Self {
        Self {
            error,
            effect_uncertain: true,
        }
    }
}

impl From<DriverError> for DriverLaunchError {
    fn from(error: DriverError) -> Self {
        Self::safe(error)
    }
}
