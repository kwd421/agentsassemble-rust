use crate::runtime::{DriverError, DriverFuture};

pub(crate) trait AntigravityTerminal: Send {
    #[expect(
        dead_code,
        reason = "PTY input stays dormant until Antigravity exposes an exact native turn receipt"
    )]
    fn read(&mut self) -> DriverFuture<'_, Result<Vec<u8>, DriverError>>;
    #[expect(
        dead_code,
        reason = "PTY output stays dormant until Antigravity exposes an exact native turn receipt"
    )]
    fn write<'a>(&'a mut self, data: &'a [u8]) -> DriverFuture<'a, Result<(), DriverError>>;
    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>>;
    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>>;
    fn request_stop(&mut self);
}
