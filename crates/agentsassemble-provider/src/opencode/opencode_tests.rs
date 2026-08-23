use tokio::io::AsyncWriteExt;

use super::{
    isolated_config_root, isolated_environment, observe_startup, server_arguments, server_password,
};

#[test]
fn server_launch_disables_external_and_project_configuration() {
    let arguments = server_arguments(43123);
    assert!(arguments.iter().any(|argument| argument == "--pure"));

    let root = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create OpenCode config fixture: {error}"));
    let password = server_password();
    let environment = isolated_environment(root.path(), &password)
        .unwrap_or_else(|_| panic!("build OpenCode environment"));
    let value = |name: &str| {
        environment
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value.as_str()))
    };
    assert_eq!(value("OPENCODE_DISABLE_PROJECT_CONFIG"), Some("1"));
    assert_eq!(value("OPENCODE_PURE"), Some("1"));
    assert_eq!(value("OPENCODE_DISABLE_EXTERNAL_SKILLS"), Some("1"));
    assert_eq!(value("XDG_CONFIG_HOME"), root.path().to_str());
    assert_eq!(value("OPENCODE_SERVER_USERNAME"), Some("agentsassemble"));
    assert_eq!(value("OPENCODE_SERVER_PASSWORD"), Some(password.as_str()));
    assert_eq!(password.len(), 64);
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn isolated_config_root_is_private_on_the_running_platform() {
    let root = isolated_config_root()
        .unwrap_or_else(|error| panic!("create isolated OpenCode config root: {error:?}"));
    assert!(root.path().is_dir());
}

#[tokio::test]
async fn startup_requires_the_exact_bound_child_endpoint() {
    let expected = "opencode server listening on http://127.0.0.1:43123";
    let (mut accepted_writer, accepted_reader) = tokio::io::duplex(1024);
    let (accepted_task, accepted) = observe_startup(accepted_reader, expected.to_owned());
    accepted_writer
        .write_all(format!("{expected}\n").as_bytes())
        .await
        .unwrap_or_else(|error| panic!("write accepted line: {error}"));
    drop(accepted_writer);
    assert_eq!(accepted.await, Ok(true));
    accepted_task
        .await
        .unwrap_or_else(|error| panic!("join accepted output: {error}"));

    let (mut rejected_writer, rejected_reader) = tokio::io::duplex(1024);
    let (rejected_task, rejected) = observe_startup(rejected_reader, expected.to_owned());
    rejected_writer
        .write_all(b"opencode server listening on http://127.0.0.1:43124\n")
        .await
        .unwrap_or_else(|error| panic!("write rejected line: {error}"));
    drop(rejected_writer);
    assert_eq!(rejected.await, Ok(false));
    rejected_task
        .await
        .unwrap_or_else(|error| panic!("join rejected output: {error}"));
}
