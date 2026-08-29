use agentsassemble_domain::{MAX_ATTACHMENT_BYTES, canonical_message_attachment_filename};
use tauri::{WebviewWindow, ipc::InvokeBody};

use crate::caller_is_bundled_ui;

fn save_request(
    body: &InvokeBody,
    encoded_filename: Option<&str>,
) -> Result<(String, Vec<u8>), String> {
    let content = match body {
        InvokeBody::Raw(content)
            if !content.is_empty() && content.len() <= MAX_ATTACHMENT_BYTES =>
        {
            content.clone()
        }
        _ => return Err("message attachment save requires 1 byte through 10 MiB".to_owned()),
    };
    let encoded_filename =
        encoded_filename.ok_or_else(|| "message attachment save filename is missing".to_owned())?;
    let filename = url::form_urlencoded::parse(format!("filename={encoded_filename}").as_bytes())
        .find_map(|(key, value)| (key == "filename").then(|| value.into_owned()))
        .ok_or_else(|| "message attachment save filename is invalid".to_owned())?;
    if canonical_message_attachment_filename(&filename) != filename {
        return Err("message attachment save filename is not canonical".to_owned());
    }
    Ok((filename, content))
}

#[tauri::command]
pub(super) async fn save_message_attachment(
    window: WebviewWindow,
    request: tauri::ipc::Request<'_>,
) -> Result<bool, String> {
    caller_is_bundled_ui(&window)?;
    let encoded_filename = request
        .headers()
        .get("x-agentsassemble-filename")
        .and_then(|value| value.to_str().ok());
    let (filename, content) = save_request(request.body(), encoded_filename)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = rfd::FileDialog::new().set_file_name(filename).save_file() else {
            return Ok(false);
        };
        std::fs::write(path, content)
            .map(|()| true)
            .map_err(|error| format!("message attachment save failed: {error}"))
    })
    .await
    .map_err(|error| format!("message attachment save worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::MAX_ATTACHMENT_BYTES;
    use tauri::ipc::InvokeBody;

    use super::save_request;

    #[test]
    fn accepts_only_bounded_raw_bytes_and_a_canonical_name() {
        let valid = InvokeBody::Raw(b"content".to_vec());
        assert_eq!(
            save_request(&valid, Some("evidence%20file.txt")),
            Ok(("evidence file.txt".to_owned(), b"content".to_vec()))
        );
        for (body, filename) in [
            (InvokeBody::Json(serde_json::json!([1])), Some("file.txt")),
            (InvokeBody::Raw(Vec::new()), Some("file.txt")),
            (
                InvokeBody::Raw(vec![0; MAX_ATTACHMENT_BYTES + 1]),
                Some("file.txt"),
            ),
            (InvokeBody::Raw(vec![1]), Some("..%2Fsecret")),
            (InvokeBody::Raw(vec![1]), None),
        ] {
            assert!(save_request(&body, filename).is_err());
        }
    }
}
