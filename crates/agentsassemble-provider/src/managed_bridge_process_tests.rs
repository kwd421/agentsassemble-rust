//! The real private worker entry in the bound test executable.
#[tokio::test]
async fn worker_entry() {
    if std::env::var_os("AGENTSASSEMBLE_TEST_MANAGED_WORKER").as_deref()
        != Some(std::ffi::OsStr::new("1"))
    {
        return;
    }
    let result = super::run_socket().await;
    std::process::exit(i32::from(result.is_err()));
}

pub(super) fn spawn_worker() -> std::io::Result<(
    crate::guardian::GuardianLaunch,
    tokio::process::Child,
    tokio::net::UnixStream,
)> {
    let launch = crate::guardian::GuardianLaunch::test_harness()?;
    let (parent, child) = std::os::unix::net::UnixStream::pair()?;
    parent.set_nonblocking(true)?;
    let parent = tokio::net::UnixStream::from_std(parent)?;
    let worker = launch.managed_command(child.into())?.spawn()?;
    Ok((launch, worker, parent))
}
