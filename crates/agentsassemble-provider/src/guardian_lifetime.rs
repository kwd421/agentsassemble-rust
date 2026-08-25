use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

pub(crate) const READY: &str = "AGENTSASSEMBLE_GUARDIAN_LIFETIME_READY";
pub(crate) const CONTINUE: &[u8] = b"AGENTSASSEMBLE_GUARDIAN_CONTINUE\n";

pub(crate) fn accept_handoff(lease_path: &Path, lease_token: &str) -> io::Result<File> {
    let lifetime = crate::runtime_lease::open_provider_lifetime_lease(lease_path, lease_token)?;
    writeln!(io::stdout().lock(), "{READY}")?;
    io::stdout().lock().flush()?;
    let mut handoff = [0_u8; CONTINUE.len()];
    io::stdin().lock().read_exact(&mut handoff)?;
    if handoff != CONTINUE {
        return Err(io::Error::other(
            "provider guardian lifetime handoff was not authorized",
        ));
    }
    Ok(lifetime)
}
