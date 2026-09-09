use std::{collections::HashMap, sync::Arc, time::Instant};

use agentsassemble_domain::{LocalResourceProcess, LocalResourceStatus};
use chrono::Utc;
use sysinfo::{CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub(crate) struct LocalResources(Arc<Mutex<Sampler>>);

#[derive(Default)]
struct Sampler {
    system: System,
    cpu_sample: Option<Instant>,
    starts: HashMap<Pid, u64>,
}

impl LocalResources {
    pub(crate) async fn read(&self) -> Result<LocalResourceStatus, ()> {
        // Acquire before dispatch: one OS read, no blocking-pool mutex waiters.
        // The owned guard survives a disconnected HTTP caller until the read ends.
        let mut sampler = Arc::clone(&self.0).lock_owned().await;
        tokio::task::spawn_blocking(move || sampler.read())
            .await
            .map_err(|_| ())?
    }
}

impl Sampler {
    fn read(&mut self) -> Result<LocalResourceStatus, ()> {
        let now = Instant::now();
        let elapsed = self.cpu_sample.map(|previous| now.duration_since(previous));
        let sample_cpu = elapsed.is_none_or(|value| value >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        self.system.refresh_memory();
        self.system.refresh_cpu_list(CpuRefreshKind::nothing());
        let mut refresh = ProcessRefreshKind::nothing().without_tasks().with_memory();
        if sample_cpu {
            refresh = refresh.with_cpu();
        }
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
        let own_pid = Pid::from_u32(std::process::id());
        if self.system.process(own_pid).is_none() {
            return Err(());
        }
        let cpu_sample_seconds = sample_cpu
            .then_some(elapsed)
            .flatten()
            .map(|v| v.as_secs_f64());
        let mut processes: Vec<_> = self
            .system
            .processes()
            .iter()
            .filter_map(|(pid, process)| {
                let label = process_label(
                    *pid == own_pid,
                    process.parent() == Some(own_pid),
                    process.name().to_str(),
                );
                label.map(|label| LocalResourceProcess {
                    pid: pid.as_u32(),
                    label: label.to_owned(),
                    cpu_percent: (cpu_sample_seconds.is_some()
                        && self.starts.get(pid) == Some(&process.start_time()))
                    .then(|| process.cpu_usage())
                    .filter(|value| value.is_finite() && *value >= 0.0),
                    memory_bytes: nonzero(process.memory()),
                })
            })
            .collect();
        if sample_cpu {
            self.starts = self
                .system
                .processes()
                .iter()
                .map(|(pid, process)| (*pid, process.start_time()))
                .collect();
            self.cpu_sample = Some(now);
        }
        let matching_process_count = processes.len();
        processes.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.memory_bytes.cmp(&a.memory_bytes))
                .then_with(|| a.pid.cmp(&b.pid))
        });
        processes.truncate(30);
        let total_memory_bytes = nonzero(self.system.total_memory());
        Ok(LocalResourceStatus {
            observed_at: Utc::now(),
            cpu_sample_seconds,
            cpu_count: (!self.system.cpus().is_empty()).then(|| self.system.cpus().len()),
            total_memory_bytes,
            available_memory_bytes: total_memory_bytes
                .and_then(|_| nonzero(self.system.available_memory())),
            load_average: load_average(),
            matching_process_count,
            processes,
        })
    }
}

const fn nonzero(value: u64) -> Option<u64> {
    if value == 0 { None } else { Some(value) }
}

fn process_label(own: bool, direct_child: bool, name: Option<&str>) -> Option<&'static str> {
    // Retained original related-tool scope. Output always comes from this vocabulary;
    // arbitrary process names (possibly user paths or secrets) never leave the owner.
    const RELATED: &[&str] = &[
        "antigravity",
        "antigravity-cli",
        "claude",
        "claude-code",
        "codex",
        "cursor",
        "cursor-agent",
        "deepseek",
        "grok",
        "grok-cli",
        "hermes",
        "kiro",
        "node",
        "npm",
        "python",
        "python3",
        "vite",
    ];
    if own {
        return Some("AgentsAssemble");
    }
    if direct_child {
        return Some("runtime-child");
    }
    let name = name?.strip_suffix(".exe").unwrap_or(name?);
    RELATED
        .iter()
        .copied()
        .find(|candidate| name.eq_ignore_ascii_case(candidate))
}

fn load_average() -> Option<[f64; 3]> {
    if cfg!(windows) {
        return None;
    }
    let load = System::load_average();
    let values = [load.one, load.five, load.fifteen];
    values
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
        .then_some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_first_sample_is_private_and_cpu_is_unknown() -> Result<(), ()> {
        let resources = LocalResources::default().read().await?;
        assert!(resources.cpu_sample_seconds.is_none());
        assert!(resources.processes.iter().all(|p| p.cpu_percent.is_none()));
        assert!(resources.processes.len() <= 30);
        assert!(process_label(false, false, Some("secret-token-path")).is_none());
        assert_eq!(
            process_label(false, true, Some("secret-token-path")),
            Some("runtime-child")
        );
        assert_eq!(
            process_label(false, false, Some("Codex.exe")),
            Some("codex")
        );
        Ok(())
    }
}
