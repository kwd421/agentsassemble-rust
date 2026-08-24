use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};
use url::Url;

use crate::local_runtime::{make_private_directory, make_private_file};

const CACHE_FILE: &str = "room-directory-v1.json";
const MAX_PAYLOAD_BYTES: usize = 512 * 1024;
const MAX_CACHED_ROOMS: usize = 128;

pub(crate) fn store(app: &AppHandle, payload: &str) -> Result<(), String> {
    let sanitized = sanitize(payload)?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot resolve desktop data directory: {error}"))?;
    fs::create_dir_all(&root)
        .map_err(|error| format!("cannot create {}: {error}", root.display()))?;
    make_private_directory(&root)
        .map_err(|error| format!("cannot secure {}: {error}", root.display()))?;
    let path = root.join(CACHE_FILE);
    reject_non_file(&path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    make_private_file(&file)
        .map_err(|error| format!("cannot secure {}: {error}", path.display()))?;
    file.write_all(sanitized.as_bytes())
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn reject_non_file(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(format!(
            "refusing to replace non-file room cache {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
}

fn sanitize(payload: &str) -> Result<String, String> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err("native room cache is too large".to_owned());
    }
    let parsed: Value = serde_json::from_str(payload)
        .map_err(|error| format!("native room cache is invalid JSON: {error}"))?;
    let rooms = parsed
        .as_array()
        .ok_or_else(|| "native room cache must be an array".to_owned())?;
    let sanitized = rooms
        .iter()
        .take(MAX_CACHED_ROOMS)
        .filter_map(sanitize_room)
        .collect::<Vec<_>>();
    serde_json::to_string(&sanitized)
        .map_err(|error| format!("cannot encode native room cache: {error}"))
}

fn sanitize_room(value: &Value) -> Option<Value> {
    let source = value.as_object()?;
    let meeting_id = bounded_text(source.get("meetingId"), 128);
    if meeting_id.is_empty() {
        return None;
    }
    let server_origin = safe_server_origin(source.get("serverOrigin"));
    let remote = source.get("roomOrigin").and_then(Value::as_str) == Some("remote_server")
        && server_origin.is_some();
    let label = bounded_text(source.get("label"), 80);
    let mut room = Map::new();
    for (key, value) in [
        ("id", bounded_text(source.get("id"), 128)),
        (
            "label",
            if label.is_empty() {
                meeting_id.clone()
            } else {
                label
            },
        ),
        ("meetingId", meeting_id),
        ("roomUid", bounded_text(source.get("roomUid"), 64)),
        ("serverId", bounded_text(source.get("serverId"), 64)),
        ("topic", bounded_text(source.get("topic"), 160)),
        ("shortLabel", bounded_text(source.get("shortLabel"), 4)),
        ("createdAt", bounded_text(source.get("createdAt"), 64)),
        ("tone", bounded_text(source.get("tone"), 16)),
    ] {
        if !value.is_empty() || matches!(key, "label" | "meetingId") {
            room.insert(key.to_owned(), Value::String(value));
        }
    }
    room.insert(
        "roomOrigin".to_owned(),
        Value::String(if remote { "remote_server" } else { "local" }.to_owned()),
    );
    if let Some(origin) = server_origin.filter(|_| remote) {
        room.insert("serverOrigin".to_owned(), Value::String(origin));
    }
    if let Some(appearance) = sanitize_appearance(source.get("appearance")) {
        room.insert("appearance".to_owned(), appearance);
    }
    Some(Value::Object(room))
}

fn sanitize_appearance(value: Option<&Value>) -> Option<Value> {
    let source = value?.as_object()?;
    let mut appearance = Map::new();
    for (key, limit) in [
        ("bannerPreset", 16),
        ("bannerImage", 256),
        ("iconImage", 256),
        ("iconLabel", 2),
        ("inviteScope", 16),
    ] {
        let text = bounded_text(source.get(key), limit);
        if !text.is_empty() {
            appearance.insert(key.to_owned(), Value::String(text));
        }
    }
    (!appearance.is_empty()).then_some(Value::Object(appearance))
}

fn bounded_text(value: Option<&Value>, limit: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .filter(|character| !matches!(character, '\r' | '\n' | '\t' | '\0'))
        .take(limit)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn safe_server_origin(value: Option<&Value>) -> Option<String> {
    let url = Url::parse(value?.as_str()?).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    #[test]
    fn cache_keeps_stable_server_room_identity_but_rejects_unsafe_origin() {
        let cached = super::sanitize(
            r#"[{"id":"room","label":"Room","meetingId":"general","roomUid":"room-uid","serverId":"server-id","roomOrigin":"remote_server","serverOrigin":"file:///tmp","topic":"Topic","appearance":{"bannerPreset":"forest","iconLabel":"AB"}}]"#,
        )
        .unwrap_or_else(|error| panic!("sanitize room cache: {error}"));
        let value: serde_json::Value = serde_json::from_str(&cached)
            .unwrap_or_else(|error| panic!("decode sanitized room cache: {error}"));
        assert_eq!(value[0]["roomUid"], "room-uid");
        assert_eq!(value[0]["serverId"], "server-id");
        assert_eq!(value[0]["roomOrigin"], "local");
        assert!(value[0].get("serverOrigin").is_none());
        assert_eq!(value[0]["appearance"]["bannerPreset"], "forest");
        assert_eq!(value[0]["appearance"]["iconLabel"], "AB");
    }
}
