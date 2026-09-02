use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_HTTP_CONNECTIONS: usize = 128;
const MAX_PUBLIC_HTTP_CONNECTIONS: usize = MAX_HTTP_CONNECTIONS - 1;

#[derive(Clone)]
pub(crate) struct HttpAdmission(Arc<HttpAdmissionInner>);

struct HttpAdmissionInner {
    total: Arc<Semaphore>,
    public: Arc<Semaphore>,
    total_rejections: AtomicU64,
    public_rejections: AtomicU64,
}

#[derive(Clone)]
pub(crate) struct HttpConnectionAdmission(Arc<HttpConnectionAdmissionInner>);

struct HttpConnectionAdmissionInner {
    _total_permit: OwnedSemaphorePermit,
    owner: HttpAdmission,
    public_permit: Mutex<Option<OwnedSemaphorePermit>>,
}

impl Default for HttpAdmission {
    fn default() -> Self {
        Self(Arc::new(HttpAdmissionInner {
            total: Arc::new(Semaphore::new(MAX_HTTP_CONNECTIONS)),
            public: Arc::new(Semaphore::new(MAX_PUBLIC_HTTP_CONNECTIONS)),
            total_rejections: AtomicU64::new(0),
            public_rejections: AtomicU64::new(0),
        }))
    }
}

impl HttpAdmission {
    pub(crate) fn admit(&self) -> Option<HttpConnectionAdmission> {
        let Ok(permit) = self.0.total.clone().try_acquire_owned() else {
            self.0.total_rejections.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        Some(HttpConnectionAdmission(Arc::new(
            HttpConnectionAdmissionInner {
                _total_permit: permit,
                owner: self.clone(),
                public_permit: Mutex::new(None),
            },
        )))
    }

    pub(crate) fn report_rejections(&self) {
        let total = self.0.total_rejections.load(Ordering::Relaxed);
        let public = self.0.public_rejections.load(Ordering::Relaxed);
        if total > 0 || public > 0 {
            tracing::warn!(total, public, "HTTP overload connections were rejected");
        }
    }
}

impl HttpConnectionAdmission {
    pub(crate) fn admit_public(&self) -> bool {
        let mut permit = self.0.public_permit.lock();
        if permit.is_some() {
            return true;
        }
        if let Ok(acquired) = self.0.owner.0.public.clone().try_acquire_owned() {
            *permit = Some(acquired);
            true
        } else {
            self.0
                .owner
                .0
                .public_rejections
                .fetch_add(1, Ordering::Relaxed);
            false
        }
    }
}
