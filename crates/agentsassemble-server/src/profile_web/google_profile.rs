use agentsassemble_domain::{MAX_ATTACHMENT_BYTES, UserProfile, UserProfilePatch};
use futures_util::StreamExt;
use serde::Deserialize;
use std::time::Duration;
use url::Url;

use super::{AppState, Json, ProfileHttpError, StatusCode, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InitialGoogleProfile {
    display_name: String,
    picture_url: String,
}

pub(super) async fn initialize(
    state: &AppState,
    google: InitialGoogleProfile,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let profile = state.store.local_operator_profile().await?;
    if profile.revision != 1 || !profile.avatar_image_url.is_empty() {
        return Ok(Json(json!({"profile": profile})));
    }
    let avatar = if google.picture_url.is_empty() {
        None
    } else {
        let url = picture_url(&google.picture_url)?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| unavailable())?;
        let response = client.get(url).send().await.map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(unavailable());
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_owned();
        let filename = match content_type.as_str() {
            "image/jpeg" => "google-avatar.jpg",
            "image/png" => "google-avatar.png",
            "image/webp" => "google-avatar.webp",
            _ => return Err(unavailable()),
        };
        let mut content = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| unavailable())?;
            if chunk.len() > MAX_ATTACHMENT_BYTES.saturating_sub(content.len()) {
                return Err(ProfileHttpError::bad_request(
                    "Google profile photo is too large.",
                ));
            }
            content.extend_from_slice(&chunk);
        }
        Some(
            state
                .store
                .store_local_operator_profile_attachment(filename, &content_type, content)
                .await?
                .url,
        )
    };
    let defaults = UserProfile::for_local_identity(&google.display_name, chrono::Utc::now())
        .ok_or_else(|| ProfileHttpError::bad_request("Google display name is empty."))?;
    let outcome = state
        .store
        .update_local_operator_profile(
            1,
            UserProfilePatch {
                display_name: Some(defaults.display_name),
                handle: Some(defaults.handle),
                avatar_label: Some(defaults.avatar_label),
                avatar_image_url: avatar,
                ..UserProfilePatch::default()
            },
        )
        .await?;
    state.rooms.notify_committed_events(&outcome.events).await;
    Ok(Json(json!({"profile": outcome.profile})))
}

fn picture_url(value: &str) -> Result<Url, ProfileHttpError> {
    let url = Url::parse(value)
        .map_err(|_| ProfileHttpError::bad_request("Invalid Google profile photo."))?;
    if value.len() > 2048
        || url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !matches!(
            url.host_str(),
            Some(
                "lh3.googleusercontent.com"
                    | "lh4.googleusercontent.com"
                    | "lh5.googleusercontent.com"
                    | "lh6.googleusercontent.com"
            )
        )
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ProfileHttpError::bad_request(
            "Invalid Google profile photo.",
        ));
    }
    Ok(url)
}

fn unavailable() -> ProfileHttpError {
    ProfileHttpError::new(
        StatusCode::BAD_GATEWAY,
        "google_photo_unavailable",
        "Google 프로필 사진을 가져오지 못했어요. 다시 시도해 주세요.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn imported_defaults_never_overwrite_a_later_user_edit() {
        let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("store: {error}"));
        store
            .bootstrap_local_authority("00000000-0000-4000-8000-000000000091", "Google user")
            .await
            .unwrap_or_else(|error| panic!("bootstrap: {error}"));
        let state = AppState::local(
            store,
            crate::TicketStore::new(Duration::from_secs(30), 16),
            agentsassemble_provider::ProviderCatalogService::fixed(
                agentsassemble_domain::ProviderCatalog::default(),
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("state: {error}"));
        let imported = initialize(
            &state,
            InitialGoogleProfile {
                display_name: "Google Name".into(),
                picture_url: String::new(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("import: {error:?}"));
        assert_eq!(imported.0["profile"]["display_name"], "Google Name");
        let edited = state
            .store
            .update_local_operator_profile(
                2,
                UserProfilePatch {
                    display_name: Some("My own name".into()),
                    avatar_label: Some("ME".into()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("edit: {error}"));
        let repeated = initialize(
            &state,
            InitialGoogleProfile {
                display_name: "Changed Google name".into(),
                picture_url: "https://blocked.example.test/not-fetched".into(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("repeat: {error:?}"));
        assert_eq!(repeated.0["profile"]["display_name"], "My own name");
        assert_eq!(repeated.0["profile"]["avatar_label"], "ME");
        assert_eq!(repeated.0["profile"]["revision"], edited.profile.revision);
    }

    #[test]
    fn photo_fetch_never_accepts_other_origins_or_credentials() {
        assert!(picture_url("https://lh3.googleusercontent.com/fixture-avatar").is_ok());
        for value in [
            "http://lh3.googleusercontent.com/avatar",
            "https://lh3.googleusercontent.com.evil.test/avatar",
            "https://127.0.0.1/avatar",
            "https://secret@lh3.googleusercontent.com/avatar",
            "https://lh3.googleusercontent.com:444/avatar",
            "https://lh3.googleusercontent.com/avatar#secret",
        ] {
            assert!(picture_url(value).is_err(), "unsafe picture URL accepted");
        }
    }
}
