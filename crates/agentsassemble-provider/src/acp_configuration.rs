use agent_client_protocol::schema::v1::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOptions, SessionId, SetSessionConfigOptionRequest,
};

use super::{AcpClient, DriverError, protocol_error};

impl AcpClient {
    pub(super) async fn select_configuration(
        &mut self,
        session_id: &SessionId,
        options: Option<Vec<SessionConfigOption>>,
        selection: &[(String, String)],
    ) -> Result<(), DriverError> {
        let mut options = options.unwrap_or_default();
        if selection.first().map(|(key, _)| key.as_str()) != Some("model")
            || selection.iter().enumerate().any(|(index, (key, _))| {
                selection[..index]
                    .iter()
                    .any(|(previous, _)| previous == key)
            })
        {
            return self.poison(unconfirmed());
        }
        for (key, value) in selection {
            let Some(option) = unique_option(&options, key) else {
                return self.poison(unconfirmed());
            };
            if !select_contains(&option.kind, value) {
                return self.poison(unconfirmed());
            }
            if selected_value(&option.kind) == Some(value) {
                continue;
            }
            let request = SetSessionConfigOptionRequest::new(
                session_id.clone(),
                option.id.clone(),
                value.as_str(),
            );
            options = match self.connection.send_request(request).block_task().await {
                Ok(response) => response.config_options,
                Err(_) => return self.poison(protocol_error()),
            };
            if !confirmed(&options, key, value) {
                return self.poison(unconfirmed());
            }
        }
        // Later parameter changes can reset earlier settings. Only the complete
        // final receipt authorizes attachment, including when reloading a session.
        if selection
            .iter()
            .all(|(key, value)| confirmed(&options, key, value))
        {
            Ok(())
        } else {
            self.poison(unconfirmed())
        }
    }
}

fn unique_option<'a>(
    options: &'a [SessionConfigOption],
    key: &str,
) -> Option<&'a SessionConfigOption> {
    let mut matches = options.iter().filter(|option| {
        option.id.0.as_ref() == key
            || (key == "model" && option.category == Some(SessionConfigOptionCategory::Model))
    });
    let option = matches.next()?;
    matches.next().is_none().then_some(option)
}

fn confirmed(options: &[SessionConfigOption], key: &str, value: &str) -> bool {
    unique_option(options, key).is_some_and(|option| {
        selected_value(&option.kind) == Some(value) && select_contains(&option.kind, value)
    })
}

fn select_contains(kind: &SessionConfigKind, value: &str) -> bool {
    let SessionConfigKind::Select(select) = kind else {
        return false;
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .any(|option| option.value.0.as_ref() == value),
        SessionConfigSelectOptions::Grouped(groups) => groups.iter().any(|group| {
            group
                .options
                .iter()
                .any(|option| option.value.0.as_ref() == value)
        }),
        _ => false,
    }
}

fn selected_value(kind: &SessionConfigKind) -> Option<&str> {
    let SessionConfigKind::Select(select) = kind else {
        return None;
    };
    Some(&select.current_value.0)
}

const fn unconfirmed() -> DriverError {
    DriverError::new(
        "provider_model_unconfirmed",
        "The ACP provider did not confirm the complete selected model configuration.",
    )
}

#[cfg(test)]
#[path = "acp_configuration_tests.rs"]
mod tests;
