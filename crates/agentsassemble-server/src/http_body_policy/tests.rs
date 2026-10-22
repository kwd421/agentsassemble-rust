use super::*;
use crate::http_api::{decode_json_body, decode_upload_body};
use axum::body::Bytes;
use serde_json::Value;

async fn upload(request: Request, limit: usize) -> Result<Value, crate::upload_body_budget::Error> {
    let policy = request
        .extensions()
        .get::<Policy>()
        .unwrap_or_else(|| panic!("router policy"))
        .clone();
    decode_upload_body(
        request,
        limit,
        &policy,
        &crate::upload_body_budget::Budget::default(),
        "test",
    )
    .await
    .map(|(value, _reservation)| value)
}

fn paced(gap: u64, bytes: usize, count: usize) -> Body {
    Body::from_stream(futures_util::stream::unfold(0, move |index| async move {
        if index > count {
            return None;
        }
        tokio::time::sleep(Duration::from_secs(gap)).await;
        let data = if index == count {
            Bytes::from_static(b"null")
        } else {
            Bytes::from(vec![b' '; bytes])
        };
        Some((Ok::<_, std::io::Error>(data), index + 1))
    }))
}

#[tokio::test(start_paused = true)]
async fn upload_progress_outlives_control_deadline_but_not_idle_or_trickle_bounds() {
    let started = Instant::now();
    let slow = wrap(Request::new(paced(20, 16 * 1024, 12)));
    assert_eq!(upload(slow, 1024 * 1024).await, Ok(Value::Null));
    assert!(started.elapsed() > Duration::from_mins(4));

    for (body, elapsed) in [
        (
            Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>()),
            30,
        ),
        (paced(10, 0, 100), 30),
        (paced(20, 1, 100), 120),
        (paced(20, 16 * 1024, 200), 3600),
    ] {
        let started = Instant::now();
        assert_eq!(
            upload(wrap(Request::new(body)), 10 * 1024 * 1024).await,
            Err(crate::upload_body_budget::Error::Body(
                BodyDecodeError::RequestTimeout
            ))
        );
        assert_eq!(started.elapsed(), Duration::from_secs(elapsed));
    }
    assert_eq!(
        upload(wrap(Request::new(Body::from("12345"))), 4).await,
        Err(crate::upload_body_budget::Error::Body(
            BodyDecodeError::PayloadTooLarge
        ))
    );
}

#[tokio::test(start_paused = true)]
async fn control_and_late_authorization_keep_short_deadline() {
    let started = Instant::now();
    assert_eq!(
        decode_json_body::<Value>(wrap(Request::new(paced(20, 1, 1))), 1024).await,
        Err(BodyDecodeError::RequestTimeout)
    );
    assert_eq!(started.elapsed(), CONTROL);
    let request = wrap(Request::new(Body::from("null")));
    tokio::time::advance(CONTROL).await;
    assert_eq!(
        upload(request, 1024).await,
        Err(crate::upload_body_budget::Error::Body(
            BodyDecodeError::RequestTimeout
        ))
    );
}
