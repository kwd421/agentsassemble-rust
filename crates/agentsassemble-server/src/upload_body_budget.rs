//! Host-owned retained-body admission, independent of encrypted queue credits.
use crate::http_api::BodyDecodeError;
use axum::{extract::Request, http::header};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const HOST_BYTES: usize = 512 * 1024 * 1024;
const COPIES: usize = 4;
const PER_PRINCIPAL: usize = 2;
pub(crate) const ERROR_CODE: &str = "upload_body_budget_exhausted";

#[derive(Clone)]
pub(crate) struct Budget {
    bytes: Arc<Semaphore>,
    principals: Arc<Mutex<HashMap<String, usize>>>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: Arc::new(Semaphore::new(HOST_BYTES)),
            principals: Arc::default(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Body(BodyDecodeError),
    Exhausted,
}
impl From<BodyDecodeError> for Error {
    fn from(error: BodyDecodeError) -> Self {
        Self::Body(error)
    }
}

pub(crate) struct Reservation {
    _bytes: OwnedSemaphorePermit,
    principals: Arc<Mutex<HashMap<String, usize>>>,
    principal: Option<String>,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let Some(principal) = &self.principal else {
            return;
        };
        let mut principals = self.principals.lock().expect("upload budget lock");
        if let Some(count) = principals.get_mut(principal) {
            *count -= 1;
            if *count == 0 {
                principals.remove(principal);
            }
        }
    }
}
impl Budget {
    pub(crate) fn reserve(
        &self,
        principal: Option<&str>,
        request: &Request,
        maximum: usize,
    ) -> Result<(usize, Reservation), Error> {
        let length = request
            .headers()
            .get(header::CONTENT_LENGTH)
            .map(|value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok())
                    .ok_or(BodyDecodeError::PayloadTooLarge)
            })
            .transpose()?
            .unwrap_or(maximum);
        if length > maximum {
            return Err(BodyDecodeError::PayloadTooLarge.into());
        }
        // The same length is also the read limit, so a lying virtual HTTP client
        // cannot reserve a small Content-Length and send a route-sized body.
        let weight = length
            .checked_mul(COPIES)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(Error::Exhausted)?;
        let mut principals = self.principals.lock().expect("upload budget lock");
        if principal.is_some_and(|principal| {
            principals.get(principal).copied().unwrap_or(0) >= PER_PRINCIPAL
        }) {
            return Err(Error::Exhausted);
        }
        let bytes = self
            .bytes
            .clone()
            .try_acquire_many_owned(weight)
            .map_err(|_| Error::Exhausted)?;
        if let Some(principal) = principal {
            *principals.entry(principal.to_owned()).or_default() += 1;
        }
        Ok((
            length,
            Reservation {
                _bytes: bytes,
                principals: self.principals.clone(),
                principal: principal.map(str::to_owned),
            },
        ))
    }
}

#[cfg(test)]
mod tests;
