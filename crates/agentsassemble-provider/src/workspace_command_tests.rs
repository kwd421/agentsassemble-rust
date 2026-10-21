use super::{MAX_STREAM_CHARACTERS, NAME, bounded, obviously_destructive, schema};

#[test]
fn obvious_disk_or_home_erasure_is_refused_on_both_shells() {
    for command in [
        "rm -rf /",
        "sudo rm -rf / --no-preserve-root",
        "rm   -rf   ~",
        "rm -rf $HOME/",
        "diskutil eraseDisk APFS x disk2",
        "Format-Volume -DriveLetter D",
        "Remove-Item -Recurse -Force C:\\",
        "format C: /q",
    ] {
        assert!(obviously_destructive(command), "{command}");
    }
    for command in [
        "rm -rf ./target",
        "cargo test",
        "Remove-Item -Recurse .\\dist",
        "ls -la /",
    ] {
        assert!(!obviously_destructive(command), "{command}");
    }
}

#[test]
fn long_output_keeps_its_end() {
    let long = format!("{}END", "x".repeat(MAX_STREAM_CHARACTERS + 10));
    let (text, truncated) = bounded(long.as_bytes());
    assert!(truncated);
    assert!(text.ends_with("END"));
    assert_eq!(text.chars().count(), MAX_STREAM_CHARACTERS);
    assert_eq!(bounded(b"ok"), ("ok".to_owned(), false));
}

#[test]
fn schema_names_the_tool_and_this_computer_shell() {
    let value = schema();
    assert_eq!(value["function"]["name"], NAME);
    let description = value["function"]["description"]
        .as_str()
        .unwrap_or_default();
    let shell = if cfg!(windows) {
        "PowerShell"
    } else if cfg!(target_os = "macos") {
        "zsh"
    } else {
        "sh"
    };
    assert!(description.contains(shell), "{description}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_command_runs_in_the_given_folder() {
    let folder = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let output = crate::process::run_in_directory(
        "/bin/sh",
        &["-c", "pwd"],
        folder.path(),
        std::time::Duration::from_secs(10),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap_or_else(|_| panic!("run failed"));
    let text = String::from_utf8_lossy(&output.stdout);
    let canonical = std::fs::canonicalize(folder.path()).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        text.contains(canonical.to_string_lossy().as_ref()),
        "{text}"
    );
}

#[test]
fn direction_overrides_and_invisible_characters_are_refused() {
    for character in ['\u{202E}', '\u{2066}', '\u{200B}', '\u{FEFF}'] {
        assert!(super::hidden_format(character), "{character:?}");
    }
    assert!(!super::hidden_format('가'));
    assert!(!super::hidden_format('|'));
}

/// A turn interrupt drops the run; what the shell started must not outlive it.
#[cfg(unix)]
#[tokio::test]
async fn a_dropped_run_takes_the_programs_it_started_with_it() {
    let folder = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let cancellation = tokio_util::sync::CancellationToken::new();
    let mut run = Box::pin(crate::process::run_in_directory(
        "/bin/sh",
        &["-c", "sleep 30 & echo $! > child; wait"],
        folder.path(),
        std::time::Duration::from_mins(1),
        &cancellation,
    ));
    let started = folder.path().join("child");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let pid = loop {
        tokio::select! {
            _ = &mut run => panic!("the run ended before it was dropped"),
            () = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
        }
        if let Some(pid) = std::fs::read_to_string(&started)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            break pid;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the shell never started its child"
        );
    };
    drop(run);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while rustix::process::test_kill_process(pid).is_ok() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the started program outlived its run"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
