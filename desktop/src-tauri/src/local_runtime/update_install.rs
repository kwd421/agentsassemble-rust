use std::sync::atomic::Ordering;

use super::LocalRuntime;

impl LocalRuntime {
    pub(crate) fn update_in_progress(&self) -> bool {
        self.updating.load(Ordering::SeqCst)
    }

    pub(super) fn ensure_not_updating(&self) -> Result<(), String> {
        if self.update_in_progress() {
            Err("업데이트 설치 중에는 서버나 로그인을 시작할 수 없어요.".into())
        } else {
            Ok(())
        }
    }

    pub(crate) fn cancel_update(&self) {
        self.updating.store(false, Ordering::SeqCst);
    }

    pub(crate) fn prepare_update(&self) -> Result<(), String> {
        if self.updating.swap(true, Ordering::SeqCst) {
            return Err("이미 업데이트를 설치하고 있어요.".into());
        }
        let result = self.stop_for_update();
        if result.is_err() {
            self.cancel_update();
        }
        result
    }

    fn stop_for_update(&self) -> Result<(), String> {
        // Admission checks run under each same slot lock. A call already in flight
        // finishes before shutdown, and no waiting call can recreate the runtime.
        for slot in [&self.login_process, &self.process] {
            let mut process = slot
                .lock()
                .map_err(|_| "서버 상태를 확인하지 못해 업데이트를 중단했어요.".to_owned())?;
            if let Some(runtime) = process.as_mut() {
                runtime.control.take();
                crate::runtime_supervisor::terminate_owned_supervisor_checked(&mut runtime.child)
                    .map_err(|_| {
                    "서버 종료를 확인하지 못해 설치하지 않았어요. 앱을 다시 시작해 주세요."
                        .to_owned()
                })?;
                *process = None;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::LocalRuntime;

    #[test]
    fn installation_closes_admission_until_failure_releases_it() {
        let runtime = LocalRuntime::default();
        assert!(runtime.ensure_not_updating().is_ok());
        assert!(runtime.prepare_update().is_ok());
        assert!(runtime.ensure_not_updating().is_err());
        assert!(runtime.prepare_update().is_err());
        assert!(runtime.update_in_progress());
        runtime.cancel_update();
        assert!(runtime.ensure_not_updating().is_ok());
    }

    #[test]
    fn unconfirmed_shutdown_blocks_installation() {
        let runtime = std::sync::Arc::new(LocalRuntime::default());
        let owner = runtime.clone();
        let poisoned = std::thread::spawn(move || {
            let _lock = owner
                .process
                .lock()
                .unwrap_or_else(|_| panic!("fixture lock"));
            panic!("simulate failed runtime owner");
        });
        assert!(poisoned.join().is_err());
        assert!(runtime.prepare_update().is_err());
        assert!(!runtime.update_in_progress());
    }
}
