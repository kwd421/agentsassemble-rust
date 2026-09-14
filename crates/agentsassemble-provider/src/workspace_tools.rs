//! Execution-bound approval and retained filesystem task custody.
use std::{io, time::Duration};

use agentsassemble_domain::{
    DurableAgentSession, ProviderRequest, ProviderRequestKind, ProviderRequestOption,
    ProviderRequestPrompt, ProviderRequestResolution,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    driver::{DriverError, ProviderTurnRequest},
    workspace_files::{self, FileOperation},
};

const FAILED: DriverError = DriverError::new(
    "workspace_tool_failed",
    "The workspace file operation failed or was denied.",
);
const CLEANUP: DriverError = DriverError::new(
    "workspace_cleanup_unconfirmed",
    "The workspace file operation has not confirmed cleanup.",
);

#[derive(Default)]
pub(crate) struct WorkspaceTools {
    task: Option<JoinHandle<io::Result<Value>>>,
    cancellation: CancellationToken,
}

impl Drop for WorkspaceTools {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl WorkspaceTools {
    async fn run(
        &mut self,
        operation: impl FnOnce(CancellationToken) -> io::Result<Value> + Send + 'static,
    ) -> Result<Value, DriverError> {
        if self.task.is_some() {
            return Err(CLEANUP);
        }
        self.cancellation = CancellationToken::new();
        let cancellation = self.cancellation.clone();
        self.task = Some(tokio::task::spawn_blocking(move || operation(cancellation)));
        let result = self.task.as_mut().ok_or(CLEANUP)?.await;
        self.task = None;
        result.map_err(|_| FAILED)?.map_err(|_| FAILED)
    }

    pub(crate) async fn cleanup(&mut self) -> Result<(), DriverError> {
        self.cancellation.cancel();
        if let Some(task) = self.task.as_mut() {
            let joined = tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .map_err(|_| CLEANUP)?;
            self.task = None;
            // Cancellation can make the operation fail; a joined task is quiescent.
            let _operation_result = joined.map_err(|_| FAILED)?;
        }
        Ok(())
    }

    pub(crate) fn pending(&self) -> bool {
        self.task.is_some()
    }

    pub(crate) async fn execute(
        &mut self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
        name: &str,
        arguments: &str,
        effect_uncertain: &mut bool,
    ) -> Result<String, DriverError> {
        if session.public.permission_mode != "workspace_write" {
            return Err(FAILED);
        }
        let mut arguments: serde_json::Map<String, Value> =
            serde_json::from_str(arguments).map_err(|_| FAILED)?;
        if arguments
            .insert("operation".to_owned(), json!(name))
            .is_some()
        {
            return Err(FAILED);
        }
        let mut operation: FileOperation =
            serde_json::from_value(Value::Object(arguments)).map_err(|_| FAILED)?;
        operation.validate().map_err(|_| FAILED)?;
        let workspace = session.workspace.clone();
        let identity = session.workspace_identity.clone();
        let prepared_workspace = workspace.clone();
        let prepared_identity = identity.clone();
        let prepared = self
            .run(move |_| {
                let root = workspace_files::bind(&prepared_workspace, &prepared_identity)?;
                operation.resolve(&root)?;
                let before = if matches!(operation, FileOperation::Replace { .. }) {
                    Some(workspace_files::read(&root, operation.path(), 1_000_000)?)
                } else {
                    None
                };
                Ok(json!({"operation":operation,"before":before}))
            })
            .await?;
        let operation: FileOperation =
            serde_json::from_value(prepared["operation"].clone()).map_err(|_| FAILED)?;
        let before = prepared["before"].as_str().map(str::to_owned);
        let path = operation.path().to_owned();
        if operation.writes() {
            approve(session, request, name, &path).await?;
            // A write may commit before its result or turn completion is observed.
            // Preserve replay uncertainty even when later filesystem/API work fails.
            *effect_uncertain = true;
        }
        let result = self
            .run(move |cancel| {
                let root = workspace_files::bind(&workspace, &identity)?;
                workspace_files::execute(&root, &operation, before.as_deref(), &cancel)
            })
            .await?;
        let encoded = result.to_string();
        if encoded.len() > workspace_files::RESULT_BYTES {
            return Err(FAILED);
        }
        Ok(encoded)
    }
}

async fn approve(
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
    name: &str,
    path: &str,
) -> Result<(), DriverError> {
    let prompt = ProviderRequest {
        provider_request_id: uuid::Uuid::new_v4(),
        request_kind: ProviderRequestKind::Permission,
        title: "작업 폴더 파일 변경".to_owned(),
        description: if name == "replace_workspace_text" {
            format!("{path} 파일의 지정한 텍스트를 바꿉니다.")
        } else {
            format!("{path} 파일을 생성하거나 덮어씁니다.")
        },
        timeout_seconds: 600,
        prompt: ProviderRequestPrompt::Option {
            options: vec![
                ProviderRequestOption {
                    id: "allow_once".to_owned(),
                    label: "이번 변경 허용".to_owned(),
                    kind: "allow_once".to_owned(),
                    description: String::new(),
                },
                ProviderRequestOption {
                    id: "deny".to_owned(),
                    label: "거부".to_owned(),
                    kind: "reject_once".to_owned(),
                    description: String::new(),
                },
            ],
        },
    };
    let mut exchange = request
        .request_ingress
        .as_ref()
        .ok_or(FAILED)?
        .open(
            &session.public.session_id,
            request.turn_generation,
            &request.execution_id,
            prompt,
        )
        .await
        .map_err(|_| FAILED)?;
    let response = exchange.receive().await.map_err(|_| FAILED)?;
    let allowed = matches!(response, ProviderRequestResolution::Option {option_id} if option_id == "allow_once");
    exchange.complete(true).await.map_err(|_| FAILED)?;
    if allowed { Ok(()) } else { Err(FAILED) }
}

pub(crate) fn names() -> [&'static str; 5] {
    [
        "list_workspace_files",
        "read_workspace_file",
        "search_workspace_text",
        "write_workspace_file",
        "replace_workspace_text",
    ]
}

pub(crate) fn schemas() -> Vec<Value> {
    let text = json!({"type":"string"});
    let line = json!({"type":"integer","minimum":1});
    names().into_iter().zip([
        (json!({"path":text}), json!([]), "List files in the selected workspace."),
        (json!({"path":text,"start_line":line,"end_line":line,"offset":{"type":"integer","minimum":0}}), json!(["path"]), "Read a UTF-8 workspace file. If next_offset is returned, repeat the same line range with that character offset to continue."),
        (json!({"path":text,"query":text}), json!(["query"]), "Search workspace files for literal text."),
        (json!({"path":text,"content":text}), json!(["path","content"]), "Create or replace a workspace file after owner approval."),
        (json!({"path":text,"old_text":text,"new_text":text,"expected_replacements":{"type":"integer","minimum":1,"maximum":100}}), json!(["path","old_text","new_text"]), "Replace exact text in a workspace file after owner approval."),
    ]).map(|(name,(properties,required,description))| json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})).collect()
}

#[cfg(test)]
#[path = "workspace_tools_tests.rs"]
mod tests;
