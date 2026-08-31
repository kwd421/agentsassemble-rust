use std::{
    env,
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

use rustix::process::{Pid, Signal};
use serde::{Deserialize, Serialize};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const GUARDIAN_FLAG: &str = "--agentsassemble-provider-guardian";
const ANCHOR_FLAG: &str = "--agentsassemble-provider-anchor";
const LAUNCHER_FLAG: &str = "--agentsassemble-provider-launcher";
const READY_PREFIX: &str = "AGENTSASSEMBLE_PROVIDER_ANCHOR=";
const PROVIDER_READY_PREFIX: &str = "AGENTSASSEMBLE_PROVIDER_READY=";
const MAX_HELPER_OUTPUT_BYTES: usize = 8 * 1024;
const MAX_PROVIDER_MANIFEST_BYTES: usize = 256 * 1024;
const TEST_MODE_ENV: &str = "AGENTSASSEMBLE_INTERNAL_GUARDIAN_MODE";
const TEST_LEASE_ENV: &str = "AGENTSASSEMBLE_INTERNAL_GUARDIAN_LEASE";
const TEST_TOKEN_ENV: &str = "AGENTSASSEMBLE_INTERNAL_GUARDIAN_TOKEN";
const TEST_FORK_POLICY_ENV: &str = "AGENTSASSEMBLE_INTERNAL_PROVIDER_FORK_POLICY";
#[cfg(test)]
const TEST_PRE_ANCHOR_SIGNAL_ENV: &str = "AGENTSASSEMBLE_INTERNAL_PRE_ANCHOR_SIGNAL";
#[cfg(test)]
const TEST_LAUNCHER_BARRIER_ENV: &str = "AGENTSASSEMBLE_INTERNAL_LAUNCHER_BARRIER";
const PROVIDER_STDIN_FD: i32 = 4;
const PROVIDER_STDOUT_FD: i32 = 5;
const PROVIDER_STDERR_FD: i32 = 6;
#[cfg(any(target_os = "linux", target_os = "android"))]
const PROVIDER_EXECUTABLE_FD: i32 = 7;
const PROVIDER_LAUNCH_FD: i32 = 8;
const PROVIDER_LIFETIME_FD: i32 = 198;

use crate::filesystem::{BoundExecutable, PrivateExecutable, bind_helper_executable_sync};
use crate::unix_process_tree::CapturedRuntimeProcesses;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderForkPolicy {
    Deny,
    AllowInGroup,
}

#[derive(Clone, Copy)]
pub(crate) enum GuardianCleanupFailure {
    ProviderState = 20,
    LeaderExited = 21,
    RuntimeCapture = 22,
    AnchorTermination = 23,
    CapturedTermination = 24,
    ProviderHistory = 25,
    AbsenceConfirmation = 26,
    CleanupReceipt = 27,
}

impl GuardianCleanupFailure {
    pub(crate) const fn from_exit_code(code: Option<i32>) -> Option<Self> {
        match code {
            Some(20) => Some(Self::ProviderState),
            Some(21) => Some(Self::LeaderExited),
            Some(22) => Some(Self::RuntimeCapture),
            Some(23) => Some(Self::AnchorTermination),
            Some(24) => Some(Self::CapturedTermination),
            Some(25) => Some(Self::ProviderHistory),
            Some(26) => Some(Self::AbsenceConfirmation),
            Some(27) => Some(Self::CleanupReceipt),
            _ => None,
        }
    }

    const fn exit_code(self) -> i32 {
        self as i32
    }
}

enum GuardianRunFailure {
    Operation,
    Cleanup(GuardianCleanupFailure),
}

impl From<io::Error> for GuardianRunFailure {
    fn from(_error: io::Error) -> Self {
        Self::Operation
    }
}

impl ProviderForkPolicy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::AllowInGroup => "allow-in-group",
        }
    }

    fn parse(value: &OsStr) -> Option<Self> {
        match value.to_str() {
            Some("deny") => Some(Self::Deny),
            Some("allow-in-group") => Some(Self::AllowInGroup),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
struct MacProviderHistory {
    watcher: Option<std::thread::JoinHandle<io::Result<bool>>>,
}

#[cfg(target_os = "macos")]
impl MacProviderHistory {
    fn watch(raw_pid: u32) -> io::Result<Self> {
        use nix::sys::event::{EvFlags, EventFilter, FilterFlag, KEvent, Kqueue};

        let queue = Kqueue::new().map_err(io::Error::other)?;
        let registration = KEvent::new(
            raw_pid as _,
            EventFilter::EVFILT_PROC,
            EvFlags::EV_ADD | EvFlags::EV_ENABLE | EvFlags::EV_CLEAR,
            FilterFlag::NOTE_FORK | FilterFlag::NOTE_EXIT,
            0,
            0,
        );
        queue
            .kevent(
                &[registration],
                &mut [],
                Some(nix::libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                }),
            )
            .map_err(io::Error::other)?;
        let watcher = std::thread::spawn(move || {
            let mut forked = false;
            let mut events = [registration];
            loop {
                let count = queue
                    .kevent(&[], &mut events, None)
                    .map_err(io::Error::other)?;
                for event in events.iter().take(count) {
                    if event.flags().contains(EvFlags::EV_ERROR) {
                        return Err(io::Error::other(
                            "macOS provider lineage watcher reported an error",
                        ));
                    }
                    let flags = event.fflags();
                    forked |= flags.contains(FilterFlag::NOTE_FORK);
                    if flags.contains(FilterFlag::NOTE_EXIT) {
                        return Ok(forked);
                    }
                }
            }
        });
        Ok(Self {
            watcher: Some(watcher),
        })
    }

    fn finish(&mut self, fork_policy: ProviderForkPolicy) -> io::Result<()> {
        let watcher = self
            .watcher
            .take()
            .ok_or_else(|| io::Error::other("macOS provider lineage watcher was consumed"))?;
        let forked = watcher
            .join()
            .map_err(|_| io::Error::other("macOS provider lineage watcher panicked"))??;
        if forked && fork_policy == ProviderForkPolicy::Deny {
            return Err(io::Error::other(
                "provider forked before macOS descendant custody could be proven",
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct ProviderLaunch {
    executable: String,
    inherited_executable_fd: bool,
    codex_code_mode_host: Option<String>,
    arguments: Vec<String>,
    environment: Vec<(String, String)>,
    working_directory: String,
}

pub(crate) struct ProviderLaunchConfig<'a> {
    pub(crate) provider: &'a BoundExecutable,
    pub(crate) arguments: &'a [String],
    pub(crate) environment: &'a [(String, String)],
    pub(crate) working_directory: &'a Path,
    pub(crate) pipes: [OwnedFd; 3],
    pub(crate) fork_policy: ProviderForkPolicy,
    pub(crate) codex_code_mode_host: Option<&'a Path>,
}

#[derive(Clone)]
pub(crate) struct GuardianLaunch {
    executable: Arc<BoundExecutable>,
    test_harness: bool,
    #[cfg(test)]
    pre_anchor_signal: Option<PathBuf>,
    #[cfg(test)]
    launcher_barrier: Option<PathBuf>,
}

impl GuardianLaunch {
    pub(crate) fn production(executable: &Path) -> io::Result<Self> {
        Ok(Self {
            executable: Arc::new(bind_helper_executable_sync(executable)?),
            test_harness: false,
            #[cfg(test)]
            pre_anchor_signal: None,
            #[cfg(test)]
            launcher_barrier: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn test_harness() -> io::Result<Self> {
        Ok(Self {
            executable: Arc::new(bind_helper_executable_sync(&reexecution_path()?)?),
            test_harness: true,
            pre_anchor_signal: None,
            launcher_barrier: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn test_harness_with_pre_anchor_signal(signal: PathBuf) -> io::Result<Self> {
        let mut launch = Self::test_harness()?;
        launch.pre_anchor_signal = Some(signal);
        Ok(launch)
    }

    #[cfg(test)]
    pub(crate) fn test_harness_with_launcher_barrier(barrier: PathBuf) -> io::Result<Self> {
        let mut launch = Self::test_harness()?;
        launch.launcher_barrier = Some(barrier);
        Ok(launch)
    }

    pub(crate) fn guardian_command(
        &self,
        lease_path: &Path,
        lease_token: &str,
        config: ProviderLaunchConfig<'_>,
    ) -> io::Result<tokio::process::Command> {
        use command_fds::FdMapping;

        let ProviderLaunchConfig {
            provider,
            arguments: provider_arguments,
            environment: provider_environment,
            working_directory,
            pipes: [provider_stdin, provider_stdout, provider_stderr],
            fork_policy,
            codex_code_mode_host,
        } = config;

        let mut command = tokio::process::Command::new(self.executable.launch_path());
        crate::process::sanitize_std_environment(command.as_std_mut());
        self.configure(
            command.as_std_mut(),
            HelperMode::Guardian,
            lease_path,
            lease_token,
        );
        if self.test_harness {
            command.env(TEST_FORK_POLICY_ENV, fork_policy.as_str());
        } else {
            command.arg(fork_policy.as_str());
        }
        #[cfg(test)]
        if let Some(signal) = &self.pre_anchor_signal {
            command.env(TEST_PRE_ANCHOR_SIGNAL_ENV, signal);
        }
        #[cfg(test)]
        if let Some(barrier) = &self.launcher_barrier {
            command.env(TEST_LAUNCHER_BARRIER_ENV, barrier);
        }
        let provider_launch = ProviderLaunch {
            executable: provider.launch_path().to_owned(),
            inherited_executable_fd: provider.requires_inherited_executable_fd(),
            codex_code_mode_host: codex_code_mode_host.map(|path| path.to_string_lossy().into()),
            arguments: provider_arguments.to_vec(),
            environment: provider_environment.to_vec(),
            working_directory: working_directory
                .to_str()
                .ok_or_else(|| io::Error::other("provider working directory is not UTF-8"))?
                .to_owned(),
        };
        let encoded = serde_json::to_string(&provider_launch).map_err(io::Error::other)?;
        if encoded.len() > MAX_PROVIDER_MANIFEST_BYTES {
            return Err(io::Error::other(
                "provider launch manifest exceeded its bound",
            ));
        }
        let (manifest, mut manifest_input) = std::io::pipe()?;
        manifest_input.write_all(encoded.as_bytes())?;
        drop(manifest_input);
        let mappings = vec![
            FdMapping {
                parent_fd: provider_stdin,
                child_fd: PROVIDER_STDIN_FD,
            },
            FdMapping {
                parent_fd: provider_stdout,
                child_fd: PROVIDER_STDOUT_FD,
            },
            FdMapping {
                parent_fd: provider_stderr,
                child_fd: PROVIDER_STDERR_FD,
            },
            FdMapping {
                parent_fd: manifest.into(),
                child_fd: PROVIDER_LAUNCH_FD,
            },
        ];
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let mappings = {
            let mut mappings = mappings;
            mappings.push(FdMapping {
                parent_fd: provider.try_clone_file()?.into(),
                child_fd: PROVIDER_EXECUTABLE_FD,
            });
            mappings
        };
        self.executable
            .configure_std_command_with_mappings(command.as_std_mut(), mappings)?;
        command.process_group(0);
        Ok(command)
    }

    pub(crate) fn stage_companion(&self, name: &str) -> io::Result<PrivateExecutable> {
        self.executable.stage_private_companion(name)
    }

    fn anchor_command(&self, lease_path: &Path, lease_token: &str) -> io::Result<Command> {
        let mut command = Command::new(self.executable.launch_path());
        command.env_clear();
        command.env(crate::unix_process_tree::RUNTIME_TOKEN_ENV, lease_token);
        self.configure(&mut command, HelperMode::Anchor, lease_path, lease_token);
        self.executable.configure_std_command(&mut command)?;
        Ok(command)
    }

    fn provider_launcher_command(
        &self,
        lease_path: &Path,
        lease_token: &str,
        process_group: Pid,
    ) -> io::Result<Command> {
        use command_fds::FdMapping;

        let mut command = Command::new(self.executable.launch_path());
        crate::process::sanitize_std_environment(&mut command);
        self.configure(
            &mut command,
            HelperMode::ProviderLauncher,
            lease_path,
            lease_token,
        );
        #[cfg(test)]
        if let Some(barrier) = &self.launcher_barrier {
            command.env(TEST_LAUNCHER_BARRIER_ENV, barrier);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(process_group.as_raw_pid());
        let lifetime = crate::runtime_lease::open_provider_lifetime_lease(lease_path, lease_token)?;
        let mappings = vec![
            FdMapping {
                parent_fd: open_inherited_fd(PROVIDER_STDIN_FD, false)?.into(),
                child_fd: PROVIDER_STDIN_FD,
            },
            FdMapping {
                parent_fd: open_inherited_fd(PROVIDER_STDOUT_FD, true)?.into(),
                child_fd: PROVIDER_STDOUT_FD,
            },
            FdMapping {
                parent_fd: open_inherited_fd(PROVIDER_STDERR_FD, true)?.into(),
                child_fd: PROVIDER_STDERR_FD,
            },
            FdMapping {
                parent_fd: lifetime.into(),
                child_fd: PROVIDER_LIFETIME_FD,
            },
            FdMapping {
                parent_fd: open_inherited_fd(PROVIDER_LAUNCH_FD, false)?.into(),
                child_fd: PROVIDER_LAUNCH_FD,
            },
        ];
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let mappings = {
            let mut mappings = mappings;
            mappings.push(FdMapping {
                parent_fd: open_inherited_fd(PROVIDER_EXECUTABLE_FD, false)?.into(),
                child_fd: PROVIDER_EXECUTABLE_FD,
            });
            mappings
        };
        self.executable
            .configure_std_command_with_mappings(&mut command, mappings)?;
        Ok(command)
    }

    fn configure(
        &self,
        command: &mut Command,
        mode: HelperMode,
        lease_path: &Path,
        lease_token: &str,
    ) {
        if self.test_harness {
            command
                .args([
                    OsStr::new("--exact"),
                    OsStr::new("runtime::tests::provider_guardian_entry"),
                    OsStr::new("--nocapture"),
                ])
                .env(
                    TEST_MODE_ENV,
                    match mode {
                        HelperMode::Guardian => "guardian",
                        HelperMode::Anchor => "anchor",
                        HelperMode::ProviderLauncher => "launcher",
                    },
                )
                .env(TEST_LEASE_ENV, lease_path)
                .env(TEST_TOKEN_ENV, lease_token);
        } else {
            command.arg(match mode {
                HelperMode::Guardian => GUARDIAN_FLAG,
                HelperMode::Anchor => ANCHOR_FLAG,
                HelperMode::ProviderLauncher => LAUNCHER_FLAG,
            });
            command.arg(lease_path);
            command.arg(lease_token);
        }
    }
}

#[derive(Clone, Copy)]
enum HelperMode {
    Guardian,
    Anchor,
    ProviderLauncher,
}

pub(crate) fn reexecution_path() -> io::Result<PathBuf> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let _executable = std::fs::File::open("/proc/self/exe")?;
        Ok(PathBuf::from("/proc/self/exe"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        env::current_exe()
    }
}

pub fn run_process_helper_if_requested() -> Option<i32> {
    let mut arguments = env::args_os();
    let _ = arguments.next();
    let mode = match arguments.next().as_deref() {
        Some(value) if value == OsStr::new(GUARDIAN_FLAG) => HelperMode::Guardian,
        Some(value) if value == OsStr::new(ANCHOR_FLAG) => HelperMode::Anchor,
        Some(value) if value == OsStr::new(LAUNCHER_FLAG) => HelperMode::ProviderLauncher,
        _ => return None,
    };
    let Some(lease_path) = arguments.next().map(PathBuf::from) else {
        return Some(2);
    };
    let Some(lease_token) = arguments.next() else {
        return Some(2);
    };
    let fork_policy = match mode {
        HelperMode::Guardian => arguments
            .next()
            .as_deref()
            .and_then(ProviderForkPolicy::parse),
        HelperMode::Anchor | HelperMode::ProviderLauncher => Some(ProviderForkPolicy::Deny),
    };
    let Some(fork_policy) = fork_policy else {
        return Some(2);
    };
    if arguments.next().is_some() {
        return Some(2);
    }
    let launch = match mode {
        HelperMode::Guardian => {
            let Ok(path) = reexecution_path() else {
                return Some(1);
            };
            let Ok(launch) = GuardianLaunch::production(&path) else {
                return Some(1);
            };
            Some(launch)
        }
        HelperMode::Anchor | HelperMode::ProviderLauncher => None,
    };
    Some(run_helper(
        mode,
        &lease_path,
        &lease_token.to_string_lossy(),
        launch.as_ref(),
        fork_policy,
    ))
}

#[cfg(test)]
pub(crate) fn run_test_helper_if_requested() -> Option<i32> {
    let mode = match env::var(TEST_MODE_ENV).ok().as_deref() {
        Some("guardian") => HelperMode::Guardian,
        Some("anchor") => HelperMode::Anchor,
        Some("launcher") => HelperMode::ProviderLauncher,
        _ => return None,
    };
    let Some(lease_path) = env::var_os(TEST_LEASE_ENV).map(PathBuf::from) else {
        return Some(2);
    };
    let Some(lease_token) = env::var_os(TEST_TOKEN_ENV) else {
        return Some(2);
    };
    let launch = match mode {
        HelperMode::Guardian => {
            let Ok(mut launch) = GuardianLaunch::test_harness() else {
                return Some(1);
            };
            launch.launcher_barrier = env::var_os(TEST_LAUNCHER_BARRIER_ENV).map(PathBuf::from);
            Some(launch)
        }
        HelperMode::Anchor | HelperMode::ProviderLauncher => None,
    };
    let fork_policy = match mode {
        HelperMode::Guardian => env::var_os(TEST_FORK_POLICY_ENV)
            .as_deref()
            .and_then(ProviderForkPolicy::parse),
        HelperMode::Anchor | HelperMode::ProviderLauncher => Some(ProviderForkPolicy::Deny),
    };
    let Some(fork_policy) = fork_policy else {
        return Some(2);
    };
    Some(run_helper(
        mode,
        &lease_path,
        &lease_token.to_string_lossy(),
        launch.as_ref(),
        fork_policy,
    ))
}

fn run_helper(
    mode: HelperMode,
    lease_path: &Path,
    lease_token: &str,
    launch: Option<&GuardianLaunch>,
    fork_policy: ProviderForkPolicy,
) -> i32 {
    match mode {
        HelperMode::Guardian => match launch {
            Some(launch) => match run_guardian(lease_path, lease_token, launch, fork_policy) {
                Ok(()) => 0,
                Err(GuardianRunFailure::Operation) => 1,
                Err(GuardianRunFailure::Cleanup(failure)) => failure.exit_code(),
            },
            None => 1,
        },
        HelperMode::Anchor => i32::from(run_anchor(lease_path, lease_token).is_err()),
        HelperMode::ProviderLauncher => i32::from(run_provider_launcher(lease_token).is_err()),
    }
}

fn run_guardian(
    lease_path: &Path,
    lease_token: &str,
    launch: &GuardianLaunch,
    fork_policy: ProviderForkPolicy,
) -> Result<(), GuardianRunFailure> {
    let launch_lifetime = crate::guardian_lifetime::accept_handoff(lease_path, lease_token)?;
    #[cfg(test)]
    if let Some(signal) = env::var_os(TEST_PRE_ANCHOR_SIGNAL_ENV) {
        std::fs::write(signal, b"spawned")?;
        std::thread::sleep(Duration::from_millis(500));
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    rustix::process::set_child_subreaper(Some(rustix::process::getpid()))?;

    let mut command = launch.anchor_command(lease_path, lease_token)?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut anchor = command.spawn()?;
    let anchor_pid = anchor.id();
    let anchor_input = anchor
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("provider anchor input is unavailable"))?;
    let mut provider = None;
    #[cfg(target_os = "macos")]
    let mut provider_history = None;
    let operation = (|| {
        let anchor_output = anchor
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("provider anchor output is unavailable"))?;
        if read_ready(BufReader::new(anchor_output))? != anchor_pid {
            return Err(io::Error::other(
                "provider anchor readiness identity changed",
            ));
        }
        let anchor_group = i32::try_from(anchor_pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| io::Error::other("provider anchor pid is invalid"))?;
        let mut provider_command =
            launch.provider_launcher_command(lease_path, lease_token, anchor_group)?;
        let provider_child = provider_command.spawn()?;
        drop(provider_command);
        let provider_pid = provider_child.id();
        wait_for_launcher_stop(provider_pid)?;
        let provider_process = i32::try_from(provider_pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| io::Error::other("provider launcher pid is invalid"))?;
        if rustix::process::getpgid(Some(provider_process))? != anchor_group {
            return Err(io::Error::other("provider launcher group changed"));
        }
        #[cfg(target_os = "macos")]
        {
            provider_history = Some(MacProviderHistory::watch(provider_pid)?);
        }
        rustix::process::kill_process(provider_process, Signal::CONT)?;
        provider = Some(provider_child);
        writeln!(
            io::stdout().lock(),
            "{PROVIDER_READY_PREFIX}{anchor_pid}:{provider_pid}"
        )?;
        io::stdout().lock().flush()?;
        crate::guardian_health::serve(
            provider
                .as_mut()
                .ok_or_else(|| io::Error::other("provider child handle is unavailable"))?,
            provider_pid,
        )
    })();
    // The launched provider owns the continuing lifetime proof once the launch
    // operation has completed. Releasing the guardian's copy before cleanup
    // lets absence confirmation distinguish a dead provider from this helper.
    drop(launch_lifetime);
    let cleanup = match provider.as_mut() {
        Some(provider) => terminate_runtime(
            &mut anchor,
            provider,
            anchor_pid,
            lease_path,
            lease_token,
            fork_policy,
            #[cfg(target_os = "macos")]
            provider_history.as_mut(),
        ),
        None => terminate_anchor(&mut anchor, anchor_pid)
            .map_err(|_| GuardianCleanupFailure::AnchorTermination)
            .and_then(|()| {
                crate::runtime_lease::mark_unix_runtime_gone(lease_path, lease_token)
                    .map_err(|_| GuardianCleanupFailure::CleanupReceipt)
            }),
    };
    drop(anchor_input);
    operation.map_err(|_| GuardianRunFailure::Operation)?;
    cleanup.map_err(GuardianRunFailure::Cleanup)
}

fn run_provider_launcher(lease_token: &str) -> io::Result<()> {
    let mut encoded = String::new();
    open_inherited_fd(PROVIDER_LAUNCH_FD, false)?
        .take((MAX_PROVIDER_MANIFEST_BYTES + 1) as u64)
        .read_to_string(&mut encoded)?;
    if encoded.len() > MAX_PROVIDER_MANIFEST_BYTES {
        return Err(io::Error::other(
            "provider launch manifest exceeded its bound",
        ));
    }
    let launch = serde_json::from_str::<ProviderLaunch>(&encoded)
        .map_err(|_| io::Error::other("provider launch manifest is invalid"))?;
    if launch.executable.is_empty()
        || launch.arguments.len() > 256
        || launch.environment.len() > 64
        || launch.working_directory.len() > 4096
        || launch
            .codex_code_mode_host
            .as_ref()
            .is_some_and(|path| path.is_empty() || path.len() > 4096)
        || launch.environment.iter().any(|(name, value)| {
            name.is_empty()
                || name.len() > 128
                || value.len() > 4096
                || name.contains(['=', '\0'])
                || value.contains('\0')
        })
    {
        return Err(io::Error::other("provider launch manifest is invalid"));
    }
    let working_directory = Path::new(&launch.working_directory);
    if !working_directory.is_absolute()
        || std::fs::canonicalize(working_directory)? != working_directory
        || !working_directory.metadata()?.is_dir()
    {
        return Err(io::Error::other(
            "provider working directory authority is invalid",
        ));
    }
    let pid = rustix::process::getpid();
    rustix::process::kill_process(pid, Signal::STOP)?;
    #[cfg(test)]
    wait_at_launcher_barrier(&launch)?;
    let code_mode_host = launch
        .codex_code_mode_host
        .as_deref()
        .map(|executable| {
            crate::codex_code_mode_host::CodexCodeModeHost::start(
                Path::new(executable),
                working_directory,
                lease_token,
                open_inherited_fd(PROVIDER_STDERR_FD, true)?,
            )
        })
        .transpose()?;
    let mut command = Command::new(&launch.executable);
    if let Some(host) = &code_mode_host {
        let Some((subcommand, arguments)) = launch.arguments.split_first() else {
            return Err(io::Error::other(
                "Codex app-server arguments are unavailable",
            ));
        };
        if subcommand != "app-server" {
            return Err(io::Error::other(
                "Codex code-mode host requires the app-server runtime",
            ));
        }
        command
            .arg(subcommand)
            .arg("--code-mode-host")
            .arg(host.endpoint())
            .args(arguments);
    } else {
        command.args(&launch.arguments);
    }
    command
        .current_dir(working_directory)
        .stdin(Stdio::from(open_inherited_fd(PROVIDER_STDIN_FD, false)?))
        .stdout(Stdio::from(open_inherited_fd(PROVIDER_STDOUT_FD, true)?))
        .stderr(Stdio::from(open_inherited_fd(PROVIDER_STDERR_FD, true)?));
    crate::process::sanitize_std_environment(&mut command);
    command.envs(launch.environment);
    command.env(crate::unix_process_tree::RUNTIME_TOKEN_ENV, lease_token);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use command_fds::{CommandFdExt, FdMapping};

        if launch.inherited_executable_fd {
            let selected = open_inherited_fd(PROVIDER_EXECUTABLE_FD, false)?;
            command
                .fd_mappings(vec![FdMapping {
                    parent_fd: selected.into(),
                    child_fd: 3,
                }])
                .map_err(io::Error::other)?;
        }
    }
    let error = command.exec();
    Err(error)
}

#[cfg(test)]
fn wait_at_launcher_barrier(launch: &ProviderLaunch) -> io::Result<()> {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;

    let Some(path) = env::var_os(TEST_LAUNCHER_BARRIER_ENV) else {
        return Ok(());
    };
    let mut stream = UnixStream::connect(path)?;
    serde_json::to_writer(
        &mut stream,
        &(
            launch.executable.as_str(),
            launch.codex_code_mode_host.as_deref(),
        ),
    )?;
    stream.write_all(b"\n")?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response)?;
    if response == "continue\n" {
        Ok(())
    } else {
        Err(io::Error::other("provider launcher barrier was rejected"))
    }
}

fn wait_for_launcher_stop(raw_pid: u32) -> io::Result<()> {
    let pid = i32::try_from(raw_pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("provider launcher pid is invalid"))?;
    let Some((observed, status)) =
        rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::UNTRACED)?
    else {
        return Err(io::Error::other("provider launcher did not stop"));
    };
    if observed != pid || !status.stopped() {
        return Err(io::Error::other("provider launcher readiness is invalid"));
    }
    Ok(())
}

fn open_inherited_fd(raw_fd: i32, write: bool) -> io::Result<File> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let path = format!("/proc/self/fd/{raw_fd}");
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let path = format!("/dev/fd/{raw_fd}");
    let mut options = OpenOptions::new();
    options.read(!write).write(write).open(path)
}

fn run_anchor(lease_path: &Path, lease_token: &str) -> io::Result<()> {
    let pid = rustix::process::getpid();
    if rustix::process::getpgrp() != pid {
        return Err(io::Error::other(
            "provider anchor is not its process-group leader",
        ));
    }
    let _lease = crate::runtime_lease::activate_unix_runtime_lease(lease_path, lease_token, pid)?;
    writeln!(io::stdout().lock(), "{READY_PREFIX}{}", pid.as_raw_pid())?;
    io::stdout().lock().flush()?;
    let mut buffer = [0_u8; 1024];
    while io::stdin().lock().read(&mut buffer)? != 0 {}
    rustix::process::kill_process_group(pid, Signal::KILL)?;
    Err(io::Error::other(
        "provider anchor survived its own group kill",
    ))
}

fn read_ready(mut reader: impl BufRead) -> io::Result<u32> {
    let mut retained = 0_usize;
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line)?;
        if count == 0 {
            return Err(io::Error::other("provider helper closed before readiness"));
        }
        retained = retained.saturating_add(count);
        if retained > MAX_HELPER_OUTPUT_BYTES {
            return Err(io::Error::other(
                "provider helper readiness exceeded its bound",
            ));
        }
        if let Some(value) = line.trim().strip_prefix(READY_PREFIX) {
            return value
                .parse::<u32>()
                .map_err(|_| io::Error::other("provider helper readiness is invalid"));
        }
    }
}

fn terminate_anchor(anchor: &mut Child, raw_pid: u32) -> io::Result<()> {
    let pid = i32::try_from(raw_pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("provider anchor pid is invalid"))?;
    let signal = rustix::process::kill_process_group(pid, Signal::KILL);
    let waited = anchor.wait();
    match (signal, waited) {
        (Ok(()), Ok(_)) => Ok(()),
        (_, Err(error)) => Err(error),
        (Err(error), _) => Err(error.into()),
    }
}

fn terminate_runtime(
    anchor: &mut Child,
    provider: &mut Child,
    raw_pid: u32,
    lease_path: &Path,
    lease_token: &str,
    fork_policy: ProviderForkPolicy,
    #[cfg(target_os = "macos")] provider_history: Option<&mut MacProviderHistory>,
) -> Result<(), GuardianCleanupFailure> {
    let pid = i32::try_from(raw_pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or(GuardianCleanupFailure::ProviderState)?;
    let provider_was_running = provider
        .try_wait()
        .map_err(|_| GuardianCleanupFailure::ProviderState)?
        .is_none();
    #[cfg(target_os = "macos")]
    if !provider_was_running {
        let _ = terminate_anchor(anchor, raw_pid);
        return Err(GuardianCleanupFailure::LeaderExited);
    }
    let captured =
        CapturedRuntimeProcesses::freeze(lease_path, lease_token, pid, provider_was_running)
            .map_err(|_| GuardianCleanupFailure::RuntimeCapture);
    let anchor_result =
        terminate_anchor(anchor, raw_pid).map_err(|_| GuardianCleanupFailure::AnchorTermination);
    let captured = match captured {
        Ok(captured) => captured,
        Err(failure) => {
            let _ = anchor_result;
            return Err(failure);
        }
    };
    let captured_result = captured
        .kill()
        .map_err(|_| GuardianCleanupFailure::CapturedTermination)
        .and_then(|()| {
            let _ = provider.wait();
            #[cfg(target_os = "macos")]
            provider_history
                .ok_or(GuardianCleanupFailure::ProviderHistory)?
                .finish(fork_policy)
                .map_err(|_| GuardianCleanupFailure::ProviderHistory)?;
            captured
                .confirm_gone(lease_path, lease_token, Duration::from_secs(4))
                .map_err(|_| GuardianCleanupFailure::AbsenceConfirmation)
        })
        .and_then(|()| {
            crate::runtime_lease::mark_unix_runtime_gone(lease_path, lease_token)
                .map_err(|_| GuardianCleanupFailure::CleanupReceipt)
        });
    anchor_result.and(captured_result)
}
