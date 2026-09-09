//! Local CLI verification and private persisted observations. No HTTP command execution.
use crate::owned_command::{OwnedCommandOutcome, run_owned_command};
use agentsassemble_domain::{
    ReleaseHealthCheck, ReleaseHealthReport, ReleaseHealthResult, ReleaseHealthStatus,
};
use chrono::Utc;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

struct Check {
    id: &'static str,
    label: &'static str,
    program: &'static str,
    arguments: &'static [&'static str],
}
const CHECKS: &[Check] = &[
    Check {
        id: "frontend_build",
        label: "프런트엔드 빌드",
        program: "npm",
        arguments: &["--prefix", "frontend", "run", "build"],
    },
    Check {
        id: "frontend_tests",
        label: "프런트엔드 테스트",
        program: "npm",
        arguments: &["--prefix", "frontend", "test"],
    },
    Check {
        id: "rust_check",
        label: "Rust 컴파일 검사",
        program: "cargo",
        arguments: &["check", "--workspace", "--all-targets"],
    },
    Check {
        id: "rust_tests",
        label: "Rust 테스트",
        program: "cargo",
        arguments: &["test", "--workspace", "--all-features"],
    },
    Check {
        id: "architecture",
        label: "구조·형식 게이트",
        program: "make",
        arguments: &["architecture-check", "format-check"],
    },
    Check {
        id: "git_diff",
        label: "Git 공백 검사",
        program: "git",
        arguments: &["diff", "--check"],
    },
];
const BUILD_ENVIRONMENT: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "COMSPEC",
    "LANG",
    "LC_ALL",
];
const REPORT_LIMIT: u64 = 64 * 1024;

/// The fixed checks available through the local CLI.
#[must_use]
pub fn catalog() -> Vec<ReleaseHealthCheck> {
    CHECKS
        .iter()
        .map(|check| ReleaseHealthCheck {
            id: check.id.to_owned(),
            label: check.label.to_owned(),
        })
        .collect()
}

/// Runs selected fixed checks and atomically persists their actual outcomes.
///
/// # Errors
/// Rejects invalid selections/repositories, overlapping runs or report write failures.
pub async fn run(
    repository: &Path,
    output_root: &Path,
    selected: &[String],
    timeout: Duration,
    cancellation: &CancellationToken,
) -> anyhow::Result<ReleaseHealthReport> {
    anyhow::ensure!(
        !timeout.is_zero() && timeout <= Duration::from_hours(1),
        "check timeout must be between 1 and 3600 seconds"
    );
    let checks = select(selected)?;
    let repository = repository.canonicalize()?;
    anyhow::ensure!(
        repository
            .join("crates/agentsassemble-server/Cargo.toml")
            .is_file()
            && repository.join("frontend/package.json").is_file(),
        "the selected directory is not the AgentsAssemble repository"
    );
    // CLI-only advisory lock; ordinary Cargo/Tauri ownership remains with their tools.
    let lock_root = repository.join("target");
    std::fs::create_dir_all(&lock_root)?;
    let _lock = lock_run(&lock_root.join("release-health.lock"))?;
    let mut report = ReleaseHealthReport {
        started_at: Utc::now(),
        completed_at: Utc::now(),
        results: Vec::new(),
    };
    let mut stop = false;
    for check in checks {
        let started = Instant::now();
        let status = if stop || cancellation.is_cancelled() {
            ReleaseHealthStatus::NotRun
        } else {
            execute(check, &repository, timeout, cancellation).await
        };
        stop |= matches!(
            status,
            ReleaseHealthStatus::Cancelled | ReleaseHealthStatus::CleanupUnconfirmed
        );
        report.results.push(ReleaseHealthResult {
            check_id: check.id.to_owned(),
            status,
            duration_seconds: started.elapsed().as_secs_f64(),
        });
    }
    report.completed_at = Utc::now();
    write_report(output_root, &report)?;
    Ok(report)
}

fn select(selected: &[String]) -> anyhow::Result<Vec<&'static Check>> {
    if selected.is_empty() {
        return Ok(CHECKS.iter().collect());
    }
    let mut seen = HashSet::new();
    selected
        .iter()
        .map(|id| {
            anyhow::ensure!(seen.insert(id), "duplicate release-health check");
            CHECKS
                .iter()
                .find(|check| check.id == id)
                .ok_or_else(|| anyhow::anyhow!("unknown release-health check; use list"))
        })
        .collect()
}

async fn execute(
    check: &Check,
    repository: &Path,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> ReleaseHealthStatus {
    let Ok(executable) = which::which(check.program) else {
        return ReleaseHealthStatus::Unavailable;
    };
    let args = check
        .arguments
        .iter()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>();
    match run_owned_command(
        &executable,
        &args,
        Some(repository),
        BUILD_ENVIRONMENT,
        cancellation,
        timeout,
    )
    .await
    {
        Ok(OwnedCommandOutcome::Exited(status)) if status.success() => ReleaseHealthStatus::Passed,
        Ok(OwnedCommandOutcome::Cancelled) => ReleaseHealthStatus::Cancelled,
        Ok(OwnedCommandOutcome::TimedOut) => ReleaseHealthStatus::TimedOut,
        Ok(OwnedCommandOutcome::CleanupFailed) => ReleaseHealthStatus::CleanupUnconfirmed,
        Ok(OwnedCommandOutcome::Exited(_) | OwnedCommandOutcome::WaitFailed) | Err(_) => {
            ReleaseHealthStatus::Failed
        }
    }
}

fn lock_run(path: &Path) -> anyhow::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
        anyhow::anyhow!("release-health is already running or its lock is unavailable")
    })?;
    Ok(file)
}

fn report_path(root: &Path) -> PathBuf {
    root.join("release_health/latest.json")
}

fn write_report(root: &Path, report: &ReleaseHealthReport) -> anyhow::Result<()> {
    let path = report_path(root);
    let directory = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid report directory"))?;
    std::fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&serde_json::to_vec(report)?)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path)?;
    Ok(())
}

/// Reads a bounded typed report; missing and unreadable/corrupt are distinct.
///
/// # Errors
/// Returns errors for inaccessible, oversized or invalid persisted observations.
pub fn read_report(root: &Path) -> anyhow::Result<Option<ReleaseHealthReport>> {
    let file = match File::open(report_path(root)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(REPORT_LIMIT + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= REPORT_LIMIT,
        "release-health report is too large"
    );
    let report: ReleaseHealthReport = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        !report.results.is_empty()
            && report.results.len() <= CHECKS.len()
            && report.completed_at >= report.started_at,
        "invalid release-health report"
    );
    let mut ids = HashSet::new();
    for result in &report.results {
        anyhow::ensure!(
            CHECKS.iter().any(|check| check.id == result.check_id)
                && ids.insert(&result.check_id)
                && result.duration_seconds.is_finite()
                && result.duration_seconds >= 0.0,
            "invalid release-health result"
        );
    }
    Ok(Some(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_missing_corrupt_and_persisted_are_distinct() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        assert!(read_report(root.path())?.is_none());
        let report = ReleaseHealthReport {
            started_at: Utc::now(),
            completed_at: Utc::now(),
            results: vec![ReleaseHealthResult {
                check_id: "git_diff".to_owned(),
                status: ReleaseHealthStatus::Failed,
                duration_seconds: 0.1,
            }],
        };
        write_report(root.path(), &report)?;
        assert_eq!(
            read_report(root.path())?
                .ok_or_else(|| anyhow::anyhow!("missing report"))?
                .results[0]
                .status,
            ReleaseHealthStatus::Failed
        );
        std::fs::write(report_path(root.path()), b"invalid")?;
        assert!(read_report(root.path()).is_err());
        assert!(select(&["not-a-command".to_owned()]).is_err());
        assert!(select(&["git_diff".to_owned(), "git_diff".to_owned()]).is_err());
        Ok(())
    }
}
