use agentsassemble_domain::{provider_setup_destination, provider_setup_from_url};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_deep_link::DeepLinkExt;

/// Receives OS navigation only. Opening a window never starts login or an installer.
pub(crate) fn install(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    app.deep_link().register_all()?;
    let receiver = app.clone();
    app.deep_link().on_open_url(move |event| {
        open_requested(&receiver, event.urls());
    });
    if let Some(urls) = app.deep_link().get_current()? {
        open_requested(app, urls);
    }
    Ok(())
}

fn open_requested(app: &AppHandle, urls: Vec<url::Url>) {
    for url in urls {
        let Some(destination) = provider_setup_from_url(&url) else {
            // Do not print an external URL: it may contain credentials even when rejected.
            eprintln!("provider_setup_link_rejected");
            continue;
        };
        if show(app, destination.provider_id, destination.display_name).is_err() {
            eprintln!("provider_setup_window_unavailable");
        }
    }
}

fn show(app: &AppHandle, provider_id: &str, display_name: &str) -> tauri::Result<()> {
    let label = format!("provider-setup-{provider_id}");
    if let Some(window) = app.get_webview_window(&label) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    // IDs come exclusively from the finite domain destination owner, never URL text.
    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?provider-setup={provider_id}").into()),
    )
    .title(format!("{display_name} 설정 — AgentsAssemble"))
    .inner_size(560.0, 640.0)
    .min_inner_size(360.0, 360.0)
    .build()?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn runtime_provider_discovery(
    window: WebviewWindow,
    app: tauri::AppHandle,
    provider_id: String,
    force: bool,
) -> Result<u64, String> {
    super::caller_is_bundled_ui(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::LocalRuntime>()
            .discover_provider(&app, &provider_id, force)
    })
    .await
    .map_err(|_| "Local provider discovery worker failed.".to_owned())?
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn open_provider_setup_help(
    window: WebviewWindow,
    provider_id: String,
) -> Result<(), String> {
    super::caller_is_bundled_ui(&window)?;
    let destination = provider_setup_destination(&provider_id)
        .ok_or_else(|| "이 제공자의 설치·업데이트 안내를 지원하지 않아요.".to_owned())?;
    open::that_detached(destination.help_url)
        .map_err(|_| "설치·업데이트 안내를 열지 못했어요.".to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_scheme_matches_the_domain_link_owner() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .unwrap_or_else(|error| panic!("desktop config: {error}"));
        assert_eq!(
            config["plugins"]["deep-link"]["desktop"]["schemes"],
            serde_json::json!([agentsassemble_domain::PROVIDER_SETUP_SCHEME])
        );
    }
}
