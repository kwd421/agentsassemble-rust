use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rfd::{AsyncMessageDialog, MessageButtons, MessageLevel};
use tauri::{
    AppHandle, Emitter, Manager, WebviewWindow,
    menu::{Menu, MenuItem, Submenu},
};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::mpsc;

const CHECK_INTERVAL: Duration = Duration::from_hours(6);
const MENU_ID: &str = "application-update";
const CHECK_LABEL: &str = "업데이트 확인…";

pub(crate) struct AppUpdates {
    requests: mpsc::Sender<Request>,
    state: Mutex<UpdateState>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    installing: AtomicBool,
}

#[derive(Clone, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum UpdateState {
    None,
    Available { version: String },
    Ready { version: String },
    Installing { version: String },
}

enum Request {
    Check,
    Install,
    Menu,
}

#[tauri::command]
pub(crate) async fn app_update_state(window: WebviewWindow) -> Result<UpdateState, String> {
    crate::caller_is_bundled_ui(&window)?;
    window
        .state::<AppUpdates>()
        .state
        .lock()
        .map(|state| state.clone())
        .map_err(|_| "업데이트 상태를 읽지 못했어요.".to_owned())
}

#[tauri::command]
pub(crate) async fn app_update_check(window: WebviewWindow) -> Result<(), String> {
    crate::caller_is_bundled_ui(&window)?;
    request(window.app_handle(), Request::Check)
}

#[tauri::command]
pub(crate) async fn app_update_install(window: WebviewWindow) -> Result<(), String> {
    crate::caller_is_bundled_ui(&window)?;
    if !matches!(
        app_update_state(window.clone()).await?,
        UpdateState::Ready { .. }
    ) {
        return Err("설치할 업데이트가 준비되지 않았어요.".to_owned());
    }
    request(window.app_handle(), Request::Install)
}

fn request(app: &AppHandle, request: Request) -> Result<(), String> {
    app.state::<AppUpdates>()
        .requests
        .try_send(request)
        .map_err(|_| "업데이트 작업이 진행 중이에요. 잠시 후 다시 시도해 주세요.".to_owned())
}

fn publish(app: &AppHandle, next: UpdateState) {
    let owner = app.state::<AppUpdates>();
    let mut state = owner
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *state = next;
    if let Some(window) = app.get_webview_window("main")
        && crate::caller_is_bundled_ui(&window).is_ok()
        && window.emit("application-update-state", &*state).is_err()
    {
        eprintln!("application_update_state_unavailable");
    }
}

struct ReadyUpdate {
    update: Update,
    // Keep exactly the bytes verified by the official updater. Never accept bytes
    // or an artifact URL from a WebView, and never reload an unsigned cache file.
    bytes: Vec<u8>,
}

pub(crate) fn install(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let item = MenuItem::with_id(app, MENU_ID, CHECK_LABEL, true, None::<&str>)?;
    let menu = Menu::default(app)?;
    menu.append(&Submenu::with_items(app, "업데이트", true, &[&item])?)?;
    app.set_menu(menu)?;
    let (requests, mut receiver) = mpsc::channel(1);
    app.manage(AppUpdates {
        requests,
        state: Mutex::new(UpdateState::None),
        task: Mutex::new(None),
        installing: AtomicBool::new(false),
    });
    app.on_menu_event(|app, event| {
        if event.id.as_ref() == MENU_ID {
            // Coalesce clicks while the single native update owner is busy.
            let _ = request(app, Request::Menu);
        }
    });
    let handle = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut ready = None;
        check(&handle, &item, &mut ready, false).await;
        loop {
            match tokio::time::timeout(CHECK_INTERVAL, receiver.recv()).await {
                Ok(Some(Request::Install)) => install_ready(&handle, &item, &mut ready).await,
                Ok(Some(Request::Menu)) if ready.is_some() => {
                    install_ready(&handle, &item, &mut ready).await;
                }
                Ok(Some(Request::Check | Request::Menu)) => {
                    check(&handle, &item, &mut ready, true).await;
                }
                Ok(None) => break,
                Err(_) if ready.is_none() => check(&handle, &item, &mut ready, false).await,
                Err(_) => {}
            }
        }
    });
    *app.state::<AppUpdates>()
        .task
        .lock()
        .map_err(|_| std::io::Error::other("application updater state is poisoned"))? = Some(task);
    Ok(())
}

pub(crate) fn stop(app: &AppHandle) {
    if let Some(owner) = app.try_state::<AppUpdates>()
        && let Ok(mut task) = owner.task.lock()
        && let Some(task) = task.take()
    {
        task.abort();
    }
}

pub(crate) fn installing(app: &AppHandle) -> bool {
    app.state::<AppUpdates>().installing.load(Ordering::SeqCst)
}

fn status(item: &MenuItem<tauri::Wry>, text: &str, enabled: bool) {
    if item
        .set_text(text)
        .and_then(|()| item.set_enabled(enabled))
        .is_err()
    {
        eprintln!("application_update_menu_unavailable");
    }
}

async fn check(
    app: &AppHandle,
    item: &MenuItem<tauri::Wry>,
    ready: &mut Option<ReadyUpdate>,
    manual: bool,
) {
    let Some(window) = app.get_webview_window("main") else {
        status(item, "업데이트 창을 열 수 없음", false);
        return;
    };
    status(item, "업데이트 확인 중…", false);
    if ready.is_none() {
        match download(app, item).await {
            Ok(Some(update)) => {
                publish(
                    app,
                    UpdateState::Ready {
                        version: update.update.version.clone(),
                    },
                );
                *ready = Some(update);
            }
            Ok(None) => {
                status(item, CHECK_LABEL, true);
                if manual {
                    notice(
                        &window,
                        &format!(
                            "현재 최신 버전({})을 사용하고 있어요.",
                            app.package_info().version
                        ),
                        false,
                    )
                    .await;
                }
                return;
            }
            Err(message) => {
                publish(app, UpdateState::None);
                status(item, "업데이트 확인 실패 · 다시 시도…", true);
                if manual {
                    notice(&window, message, true).await;
                }
                return;
            }
        }
    }
    let Some(pending) = ready.as_ref() else {
        return;
    };
    let label = format!("{} 설치 후 재시작…", pending.update.version);
    status(item, &label, true);
}

async fn install_ready(
    app: &AppHandle,
    item: &MenuItem<tauri::Wry>,
    ready: &mut Option<ReadyUpdate>,
) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Some(pending) = ready.take() else { return };
    app.state::<AppUpdates>()
        .installing
        .store(true, Ordering::SeqCst);
    publish(
        app,
        UpdateState::Installing {
            version: pending.update.version.clone(),
        },
    );
    status(item, "서버 종료 및 업데이트 설치 중…", false);
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let runtime = handle.state::<crate::LocalRuntime>();
        runtime.prepare_update()?;
        if pending.update.install(&pending.bytes).is_ok() {
            Ok(())
        } else {
            runtime.cancel_update();
            Err(
                "업데이트 설치에 실패했어요. 앱을 다시 시작한 뒤 업데이트를 다시 확인해 주세요."
                    .to_owned(),
            )
        }
    })
    .await;
    app.state::<AppUpdates>()
        .installing
        .store(false, Ordering::SeqCst);
    match result {
        Ok(Ok(())) => app.request_restart(),
        Ok(Err(message)) => {
            publish(app, UpdateState::None);
            status(item, "업데이트 설치 실패 · 다시 시도…", true);
            notice(&window, &message, true).await;
        }
        Err(_) => {
            publish(app, UpdateState::None);
            status(item, "업데이트 상태 확인 필요", false);
            notice(
                &window,
                "업데이트 작업 결과를 확인하지 못했어요. 앱을 다시 시작해 주세요.",
                true,
            )
            .await;
        }
    }
}

async fn download(
    app: &AppHandle,
    item: &MenuItem<tauri::Wry>,
) -> Result<Option<ReadyUpdate>, &'static str> {
    let mut builder = app.updater_builder().timeout(Duration::from_mins(15));
    if env!("AGENTSASSEMBLE_UPDATE_CHANNEL") == "test" {
        builder = builder.endpoints(vec![
            "https://github.com/kwd421/agentsassemble-rust/releases/download/desktop-test/latest.json"
                .parse().map_err(|_| "업데이트 주소를 읽지 못했어요.")?
        ]).map_err(|_| "업데이트 주소를 읽지 못했어요.")?;
    }
    let updater = builder
        .build()
        .map_err(|_| "업데이트 설정을 읽지 못했어요.")?;
    let update = tokio::time::timeout(Duration::from_secs(30), updater.check())
        .await
        .map_err(|_| "업데이트 서버 응답 시간이 초과됐어요. 나중에 다시 확인해 주세요.")?
        .map_err(
            |_| "업데이트 정보를 확인하지 못했어요. 네트워크 연결을 확인하고 다시 시도해 주세요.",
        )?;
    let Some(update) = update else {
        return Ok(None);
    };
    if update.download_url.scheme() != "https" {
        return Err("업데이트 다운로드 주소가 안전한 HTTPS 주소가 아니어서 중단했어요.");
    }
    publish(
        app,
        UpdateState::Available {
            version: update.version.clone(),
        },
    );
    status(item, "업데이트 다운로드 중…", false);
    let bytes = update.download(|_, _| {}, || {}).await.map_err(
        |_| "업데이트 다운로드 또는 서명 확인에 실패했어요. 설치하지 않았으니 다시 시도해 주세요.",
    )?;
    Ok(Some(ReadyUpdate { update, bytes }))
}

async fn notice(window: &tauri::WebviewWindow, message: &str, error: bool) {
    AsyncMessageDialog::new()
        .set_parent(window)
        .set_title("AgentsAssemble 업데이트")
        .set_description(message)
        .set_level(if error {
            MessageLevel::Error
        } else {
            MessageLevel::Info
        })
        .set_buttons(MessageButtons::Ok)
        .show()
        .await;
}
