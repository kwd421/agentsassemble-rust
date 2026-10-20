use std::path::Path;

use agentsassemble_domain::{ProviderAvailability, ProviderCatalog};

use crate::{
    filesystem::{FilesystemFailure, runtime_executable_identity},
    registration::provider_registration_by_id,
};

use super::ProviderSelectionError;

/// The built-in driver and the one alternate harness exposed by this slice.
pub(super) const BUILTIN: &str = "builtin";

pub(super) struct ReservedHarness {
    pub(super) name: String,
    pub(super) executable: String,
    pub(super) executable_identity: String,
}

pub(super) async fn resolve(
    provider: &ProviderAvailability,
    executable_required: bool,
    requested: Option<&str>,
    catalog: &ProviderCatalog,
) -> Result<ReservedHarness, ProviderSelectionError> {
    let name = requested
        .filter(|value| !value.is_empty())
        .unwrap_or(BUILTIN);
    match name {
        BUILTIN => {
            let (executable, executable_identity) =
                builtin_authority(provider, executable_required).await?;
            Ok(ReservedHarness {
                name: BUILTIN.to_owned(),
                executable,
                executable_identity,
            })
        }
        crate::opencode_harness::NAME => {
            if !matches!(
                provider.provider_kind.as_str(),
                "gemini_api" | "deepseek_api"
            ) {
                return Err(unsupported());
            }
            let (executable, executable_identity) = opencode_authority(catalog).await?;
            Ok(ReservedHarness {
                name: crate::opencode_harness::NAME.to_owned(),
                executable,
                executable_identity,
            })
        }
        _ => Err(unsupported()),
    }
}

async fn builtin_authority(
    provider: &ProviderAvailability,
    executable_required: bool,
) -> Result<(String, String), ProviderSelectionError> {
    if !executable_required {
        return if provider.executable.is_empty() && provider.executable_identity.is_empty() {
            Ok((String::new(), String::new()))
        } else {
            Err(catalog_inconsistent(
                "Provider executable authority is inconsistent.",
            ))
        };
    }
    if !Path::new(&provider.executable).is_absolute() {
        return Err(catalog_inconsistent(
            "Provider executable authority is not absolute.",
        ));
    }
    let identity =
        runtime_executable_identity(&provider.provider_kind, provider.executable.clone())
            .await
            .map_err(executable_validation_error)?;
    if identity != provider.executable_identity {
        return Err(catalog_changed());
    }
    Ok((provider.executable.clone(), identity))
}

async fn opencode_authority(
    catalog: &ProviderCatalog,
) -> Result<(String, String), ProviderSelectionError> {
    let opencode = catalog
        .providers
        .iter()
        .find(|provider| provider.id == "opencode")
        .ok_or_else(opencode_unavailable)?;
    if !opencode.startable || opencode.discovery_status != "ready" {
        return Err(opencode_unavailable());
    }
    provider_registration_by_id(&opencode.id)
        .filter(|registration| {
            registration.provider_kind == opencode.provider_kind
                && registration.runtime_kind == opencode.runtime_kind
                && registration.connection_kind == opencode.connection_kind
                && registration.configuration_authority.custom_endpoint()
                    == opencode.custom_endpoint
                && registration.configuration_authority.custom_model() == opencode.custom_model
        })
        .ok_or_else(opencode_unavailable)?;
    if !Path::new(&opencode.executable).is_absolute() {
        return Err(catalog_inconsistent(
            "OpenCode executable authority is not absolute.",
        ));
    }
    let identity =
        runtime_executable_identity(&opencode.provider_kind, opencode.executable.clone())
            .await
            .map_err(executable_validation_error)?;
    if identity != opencode.executable_identity {
        return Err(catalog_changed());
    }
    Ok((opencode.executable.clone(), identity))
}

fn unsupported() -> ProviderSelectionError {
    ProviderSelectionError::new(
        "unsupported_control",
        "Alternate execution harnesses are not available in this runtime slice.",
    )
}

fn opencode_unavailable() -> ProviderSelectionError {
    ProviderSelectionError::new(
        "opencode_harness_unavailable",
        "OpenCode is not ready in the current provider catalog; refresh discovery before choosing it.",
    )
}

fn catalog_changed() -> ProviderSelectionError {
    ProviderSelectionError::new(
        "catalog_changed",
        "Provider executable identity changed; refresh discovery.",
    )
}

fn catalog_inconsistent(message: &'static str) -> ProviderSelectionError {
    ProviderSelectionError::new("catalog_inconsistent", message)
}

fn executable_validation_error(failure: FilesystemFailure) -> ProviderSelectionError {
    let message = match failure {
        FilesystemFailure::Timeout => "Provider executable validation timed out.",
        FilesystemFailure::Failed => "Provider executable authority could not be reopened.",
    };
    catalog_inconsistent(message)
}

#[cfg(test)]
#[path = "selection_harness_tests.rs"]
mod tests;
