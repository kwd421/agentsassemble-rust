use std::{error::Error, sync::Arc};

use agentsassemble_domain::ProviderRequestResolution;
use serde_json::json;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::WorkspaceTools;
use crate::{
    ProviderRequestExchange, ProviderRequestIngress,
    driver::ProviderTurnRequest,
    filesystem::canonical_workspace,
    test_support::durable_session,
    workspace_files::{self, FileOperation},
};

#[test]
fn file_tools_preserve_boundaries_and_exact_replacement() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let root = cap_std::fs::Dir::open_ambient_dir(workspace.path(), cap_std::ambient_authority())?;
    let cancellation = CancellationToken::new();
    let run = |value, before| {
        let mut operation: FileOperation = serde_json::from_value(value)?;
        operation.validate()?;
        operation.resolve(&root)?;
        workspace_files::execute(&root, &operation, before, &cancellation)
    };
    run(
        json!({"operation":"write_workspace_file","path":"sub/file.txt","content":"first\nneedle\nlast"}),
        None,
    )?;
    let read = run(
        json!({"operation":"read_workspace_file","path":"sub/file.txt","start_line":2,"end_line":2}),
        None,
    )?;
    assert_eq!(read["content"], "needle\n");
    assert_eq!(
        run(
            json!({"operation":"search_workspace_text","query":"needle"}),
            None
        )?["matches"][0]["line"],
        2
    );
    assert_eq!(
        run(json!({"operation":"list_workspace_files"}), None)?["files"],
        json!(["sub/file.txt"])
    );
    let replace = json!({"operation":"replace_workspace_text","path":"sub/file.txt","old_text":"needle","new_text":"changed"});
    assert!(run(replace.clone(), Some("stale")).is_err());
    run(replace, Some("first\nneedle\nlast"))?;
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("sub/file.txt"))?,
        "first\nchanged\nlast"
    );
    for path in [
        "../escape",
        "/escape",
        ".git/config",
        "nested/.Hg/config",
        "C:/escape",
    ] {
        assert!(
            run(
                json!({"operation":"write_workspace_file","path":path,"content":"forbidden"}),
                None
            )
            .is_err()
        );
    }
    let protected = outside.path().join("protected");
    std::fs::write(&protected, "outside")?;
    std::fs::hard_link(&protected, workspace.path().join("alias"))?;
    run(
        json!({"operation":"write_workspace_file","path":"alias","content":"inside"}),
        None,
    )?;
    assert_eq!(std::fs::read_to_string(&protected)?, "outside");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("sub/file.txt", workspace.path().join("inside-link"))?;
        assert_eq!(
            run(
                json!({"operation":"read_workspace_file","path":"inside-link"}),
                None
            )?["content"],
            "first\nchanged\nlast"
        );
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("link"))?;
        assert!(
            run(
                json!({"operation":"read_workspace_file","path":"link/protected"}),
                None
            )
            .is_err()
        );
        assert!(run(json!({"operation":"write_workspace_file","path":"link/protected","content":"forbidden"}), None).is_err());
    }
    cancellation.cancel();
    assert!(
        run(
            json!({"operation":"write_workspace_file","path":"cancelled","content":"forbidden"}),
            None
        )
        .is_err()
    );
    assert!(!workspace.path().join("cancelled").exists());
    Ok(())
}

#[tokio::test]
async fn writes_require_exact_owner_response_and_delivery_receipt() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let mut session = durable_session("room", "session", "API", "deepseek", "model", "https");
    session.public.permission_mode = "workspace_write".to_owned();
    (session.workspace, session.workspace_identity) =
        canonical_workspace(directory.path().to_string_lossy().into_owned())
            .await
            .map_err(|_| "workspace")?;
    let (ingress, mut commands) = ProviderRequestIngress::channel(1);
    let request = ProviderTurnRequest {
        request_ingress: Some(ingress),
        turn_id: "turn".to_owned(),
        turn_generation: 9,
        execution_id: "execution".to_owned(),
        input: String::new(),
        room_observation: None,
    };
    let mut tools = WorkspaceTools::default();
    for (option, delivered, success) in [
        ("deny", true, false),
        ("allow_once", false, false),
        ("allow_once", true, true),
    ] {
        let mut uncertain = false;
        let operation = tools.execute(
            &session,
            &request,
            "write_workspace_file",
            r#"{"path":"approved.txt","content":"owner-approved"}"#,
            &mut uncertain,
        );
        let broker = async {
            let command = commands.recv().await.ok_or("request missing")?;
            assert_eq!(command.session_id, "session");
            assert_eq!(command.turn_generation, 9);
            assert_eq!(command.execution_id, "execution");
            assert!(!command.request.description.contains("owner-approved"));
            let (exchange, mut response, mut completion) = ProviderRequestExchange::channel();
            command.complete(Ok(exchange));
            response.respond(ProviderRequestResolution::Option {
                option_id: option.to_owned(),
            })?;
            assert!(completion.completion().await);
            if delivered {
                completion.finish(Ok(()));
            }
            Ok::<_, Box<dyn Error>>(())
        };
        let (result, broker) = tokio::join!(operation, broker);
        broker?;
        assert_eq!(result.is_ok(), success);
        assert_eq!(uncertain, success);
        assert_eq!(directory.path().join("approved.txt").exists(), success);
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("approved.txt"))?,
        "owner-approved"
    );
    session.public.permission_mode = "meeting_read_only".to_owned();
    assert!(
        tools
            .execute(
                &session,
                &request,
                "read_workspace_file",
                r#"{"path":"approved.txt"}"#,
                &mut false
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancellation_retains_job_until_confirmed_quiescence() -> Result<(), Box<dyn Error>> {
    let mut tools = WorkspaceTools::default();
    let entered = Arc::new(Notify::new());
    let worker_entered = entered.clone();
    let (release, released) = std::sync::mpsc::channel();
    {
        let operation = tools.run(move |cancel| {
            worker_entered.notify_one();
            released.recv().map_err(std::io::Error::other)?;
            Ok(json!({"cancelled":cancel.is_cancelled()}))
        });
        tokio::pin!(operation);
        tokio::select! {
            biased;
            result = &mut operation => panic!("job completed before release: {result:?}"),
            () = entered.notified() => {},
        }
    }
    assert!(tools.pending());
    {
        let cleanup = tools.cleanup();
        tokio::pin!(cleanup);
        let result = tokio::select! {
            biased;
            result = &mut cleanup => result,
            () = tokio::time::advance(std::time::Duration::from_secs(6)) => cleanup.await,
        };
        assert!(result.is_err());
    }
    assert!(tools.pending());
    tokio::time::resume();
    release.send(())?;
    tools.cleanup().await?;
    assert!(!tools.pending());
    Ok(())
}

#[tokio::test]
async fn file_result_budget_preserves_unicode_and_escaped_partial_results()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let mut session = durable_session("room", "session", "API", "deepseek", "model", "https");
    session.public.permission_mode = "workspace_write".to_owned();
    (session.workspace, session.workspace_identity) =
        canonical_workspace(directory.path().to_string_lossy().into_owned())
            .await
            .map_err(|_| "workspace")?;
    let request = ProviderTurnRequest {
        request_ingress: None,
        turn_id: "turn".to_owned(),
        turn_generation: 1,
        execution_id: "execution".to_owned(),
        input: String::new(),
        room_observation: None,
    };
    let mut tools = WorkspaceTools::default();
    for text in ["A".repeat(50_000), "가".repeat(50_000), "\"".repeat(80_000)] {
        std::fs::write(directory.path().join("text.txt"), &text)?;
        let first = tools
            .execute(
                &session,
                &request,
                "read_workspace_file",
                r#"{"path":"text.txt","start_line":1,"end_line":1}"#,
                &mut false,
            )
            .await
            .map_err(|e| format!("read: {}", e.code))?;
        assert!(first.len() <= 128 * 1024);
        let first: serde_json::Value = serde_json::from_str(&first)?;
        let mut content = first["content"]
            .as_str()
            .ok_or("content missing")?
            .to_owned();
        if content != text {
            assert_eq!(first["truncated"], true);
            let next = tools.execute(&session, &request, "read_workspace_file", &json!({"path":"text.txt","start_line":1,"end_line":1,"offset":first["next_offset"]}).to_string(), &mut false).await.map_err(|e| format!("continue: {}", e.code))?;
            assert!(next.len() <= 128 * 1024);
            let next: serde_json::Value = serde_json::from_str(&next)?;
            content.push_str(
                next["content"]
                    .as_str()
                    .ok_or("continued content missing")?,
            );
            assert_eq!(next["next_offset"], serde_json::Value::Null);
        }
        assert_eq!(content, text);
    }
    std::fs::write(
        directory.path().join("text.txt"),
        format!("{}\n", "가".repeat(500)).repeat(201),
    )?;
    let search = tools
        .execute(
            &session,
            &request,
            "search_workspace_text",
            r#"{"query":"가"}"#,
            &mut false,
        )
        .await
        .map_err(|e| format!("search: {}", e.code))?;
    assert!(search.len() <= 128 * 1024);
    let search: serde_json::Value = serde_json::from_str(&search)?;
    let matches = search["matches"].as_array().ok_or("matches missing")?;
    assert!(!matches.is_empty());
    assert!(matches.len() < 200);
    assert_eq!(matches[0]["text"], "가".repeat(500));
    assert_eq!(search["truncated"], true);
    Ok(())
}

#[test]
fn workspace_commit_child() -> Result<(), Box<dyn Error>> {
    let Some(path) = std::env::var_os("AA_WORKSPACE_COMMIT_TEST") else {
        return Ok(());
    };
    let root = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
    let operation = FileOperation::Replace {
        path: "file.txt".into(),
        old_text: "beta=0".into(),
        new_text: "beta=1".into(),
        expected_replacements: 1,
    };
    assert!(
        workspace_files::execute(
            &root,
            &operation,
            Some("alpha=0 beta=0"),
            &CancellationToken::new()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn workspace_commit_excludes_other_processes_and_stale_approvals() -> Result<(), Box<dyn Error>> {
    use std::fs::OpenOptions;
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("file.txt"), "alpha=0 beta=0")?;
    let root = cap_std::fs::Dir::open_ambient_dir(workspace.path(), cap_std::ambient_authority())?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(workspace.path().join(".agentsassemble-write.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    let child = || -> Result<(), Box<dyn Error>> {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "workspace_tools::tests::workspace_commit_child",
                "--nocapture",
            ])
            .env("AA_WORKSPACE_COMMIT_TEST", workspace.path())
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        Ok(())
    };
    // Parent holds the real commit boundary before comparison: another OS process
    // must fail, not reach its rename. No scheduling sleeps or model filesystem.
    child()?;
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("file.txt"))?,
        "alpha=0 beta=0"
    );
    drop(lock);
    let operation = FileOperation::Replace {
        path: "file.txt".into(),
        old_text: "alpha=0".into(),
        new_text: "alpha=1".into(),
        expected_replacements: 1,
    };
    workspace_files::execute(
        &root,
        &operation,
        Some("alpha=0 beta=0"),
        &CancellationToken::new(),
    )?;
    // The same preapproval snapshot is now stale even after exclusion is released.
    child()?;
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("file.txt"))?,
        "alpha=1 beta=0"
    );
    assert_eq!(std::fs::read_dir(workspace.path())?.count(), 2);
    Ok(())
}
