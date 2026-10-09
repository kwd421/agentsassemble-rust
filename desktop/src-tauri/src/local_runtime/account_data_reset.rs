//! Native installation lifecycle owns shutdown and the offline server reset command.
use super::{LocalRuntime, RuntimeProcess, sidecar_executable};
use std::{path::Path, sync::atomic::Ordering};
use tauri::{AppHandle, Manager};

impl LocalRuntime {
    /// # Errors
    /// Refuses concurrent maintenance, unsafe installation path or unconfirmed shutdown/reset.
    pub fn wipe_account_data(&self, app: &AppHandle) -> Result<serde_json::Value, String> {
        self.prepare_update()?; // shared maintenance admission excludes updater/start/cache writes
        let result = (|| {
            let mut process = self
                .process
                .lock()
                .map_err(|_| "서버 수명 상태를 확인하지 못했어요.".to_owned())?;
            let mut login = self
                .login_process
                .lock()
                .map_err(|_| "로그인 수명 상태를 확인하지 못했어요.".to_owned())?;
            let root = app
                .path()
                .app_data_dir()
                .map_err(|_| "이 앱의 데이터 위치를 확인하지 못했어요.".to_owned())?;
            if root
                .symlink_metadata()
                .map_err(|_| "데이터 위치를 확인하지 못했어요.".to_owned())?
                .file_type()
                .is_symlink()
            {
                return Err("이 앱의 데이터 위치가 실제 디렉터리가 아니에요.".into());
            }
            let root = root
                .canonicalize()
                .map_err(|_| "데이터 위치를 확인하지 못했어요.".to_owned())?;
            self.data_reset_stopped.store(true, Ordering::Release);
            stop_checked(&mut login)?;
            stop_checked(&mut process)?;
            let desktop = std::env::current_exe()
                .map_err(|_| "앱 실행 파일을 확인하지 못했어요.".to_owned())?;
            let executable = sidecar_executable(&desktop)?;
            run_reset(&executable, &root.join("runtime.sqlite3"))
        })();
        self.cancel_update();
        result
    }
    pub(crate) fn store_room_cache(&self, app: &AppHandle, rooms: &str) -> Result<(), String> {
        let _lease = self
            .process
            .lock()
            .map_err(|_| "서버 수명 상태를 확인하지 못했어요.".to_owned())?;
        self.ensure_not_updating()?;
        if self.data_reset_stopped.load(Ordering::Acquire) {
            return Err("데이터 삭제 후에는 방 목록을 저장하지 않아요.".into());
        }
        crate::room_directory_cache::store(app, rooms)
    }
}

fn stop_checked(slot: &mut Option<RuntimeProcess>) -> Result<(), String> {
    if let Some(runtime) = slot.as_mut() {
        runtime.control.take();
        crate::runtime_supervisor::terminate_owned_supervisor_checked(&mut runtime.child)
            .map_err(|_|"이 앱의 서버와 자식 프로세스 종료를 확인하지 못했어요. 방 데이터는 지우지 않았어요.".to_owned())?;
    }
    slot.take();
    Ok(())
}
fn run_reset(executable: &Path, database: &Path) -> Result<serde_json::Value, String> {
    let output=crate::runtime_supervisor::reset_account_data(executable,database)
        .map_err(|_|"방 데이터 삭제 결과를 확인하지 못했어요. 서버는 중지된 상태예요. 다시 실행하기 전에 결과를 확인해 주세요.".to_owned())?;
    let outcome: serde_json::Value = serde_json::from_slice(&output).map_err(|_| {
        "방 데이터 삭제 결과를 확인하지 못했어요. 서버는 중지된 상태예요.".to_owned()
    })?;
    if outcome.as_object().is_none_or(|value| {
        value.len() != 2 || !value.contains_key("reset") || !value.contains_key("cleanup_errors")
    }) || outcome["reset"] != true
        || outcome["cleanup_errors"].as_array().is_none_or(|errors| {
            errors.len() > 16
                || errors
                    .iter()
                    .any(|error| error.as_str().is_none_or(|text| text.len() > 256))
        })
    {
        return Err("방 데이터 삭제 결과가 일치하지 않아요. 서버는 중지된 상태예요.".into());
    }
    Ok(outcome)
}
