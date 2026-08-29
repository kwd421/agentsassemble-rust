use agentsassemble_domain::{MAX_ATTACHMENT_BYTES, canonical_message_attachment_filename};
use tauri::{WebviewWindow, ipc::InvokeBody};

use crate::caller_is_bundled_ui;

mod secure_replace;

use secure_replace::save_atomically;

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
        save_atomically(&path, &content)
            .map(|()| true)
            .map_err(|error| format!("message attachment save failed: {error}"))
    })
    .await
    .map_err(|error| format!("message attachment save worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use std::fs;

    use agentsassemble_domain::MAX_ATTACHMENT_BYTES;
    use tauri::ipc::InvokeBody;
    use tempfile::tempdir;

    use super::{save_atomically, save_request};

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

    #[test]
    fn atomic_save_replaces_only_the_selected_regular_path() {
        let directory = tempdir().unwrap_or_else(|error| panic!("create save directory: {error}"));
        let directory = fs::canonicalize(directory.path())
            .unwrap_or_else(|error| panic!("resolve save directory: {error}"));
        let target = directory.join("evidence.txt");
        fs::write(&target, b"previous")
            .unwrap_or_else(|error| panic!("write previous target: {error}"));

        save_atomically(&target, b"complete replacement")
            .unwrap_or_else(|error| panic!("save replacement: {error}"));

        assert_eq!(
            fs::read(&target).unwrap_or_else(|error| panic!("read replacement: {error}")),
            b"complete replacement"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_rejects_a_symlink_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap_or_else(|error| panic!("create save directory: {error}"));
        let directory = fs::canonicalize(directory.path())
            .unwrap_or_else(|error| panic!("resolve save directory: {error}"));
        let protected = directory.join("protected.txt");
        let selected = directory.join("selected.txt");
        fs::write(&protected, b"protected")
            .unwrap_or_else(|error| panic!("write protected target: {error}"));
        symlink(&protected, &selected)
            .unwrap_or_else(|error| panic!("create selected symlink: {error}"));

        assert!(save_atomically(&selected, b"replacement").is_err());
        assert_eq!(
            fs::read(&protected).unwrap_or_else(|error| panic!("read protected target: {error}")),
            b"protected"
        );
        assert!(
            fs::symlink_metadata(&selected)
                .unwrap_or_else(|error| panic!("read selected metadata: {error}"))
                .file_type()
                .is_symlink()
        );

        let linked = directory.join("linked.txt");
        fs::hard_link(&protected, &linked)
            .unwrap_or_else(|error| panic!("create selected hard link: {error}"));
        save_atomically(&linked, b"new entry")
            .unwrap_or_else(|error| panic!("replace selected hard link: {error}"));
        assert_eq!(
            fs::read(&protected).unwrap_or_else(|error| panic!("read hard-link target: {error}")),
            b"protected"
        );
        assert_eq!(
            fs::read(&linked).unwrap_or_else(|error| panic!("read hard-link replacement: {error}")),
            b"new entry"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_rejects_a_symlinked_parent_component() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap_or_else(|error| panic!("create save directory: {error}"));
        let directory = fs::canonicalize(directory.path())
            .unwrap_or_else(|error| panic!("resolve save directory: {error}"));
        let real_parent = directory.join("real");
        let selected_parent = directory.join("selected");
        fs::create_dir(&real_parent).unwrap_or_else(|error| panic!("create real parent: {error}"));
        symlink(&real_parent, &selected_parent)
            .unwrap_or_else(|error| panic!("create parent symlink: {error}"));

        assert!(save_atomically(&selected_parent.join("evidence.txt"), b"content").is_err());
        assert!(!real_parent.join("evidence.txt").exists());
    }
}
