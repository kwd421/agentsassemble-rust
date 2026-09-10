//! Opens a bundled local creation draft; a navigation never admits or starts an attendee.
use agentsassemble_protocol::AttendeeEntryPacket;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;
use uuid::Uuid;

pub(crate) fn open(app: &AppHandle, url: &Url) -> Result<(), ()> {
    let packet = decode(url).ok_or(())?;
    let id = Uuid::parse_str(&packet.request_id).map_err(|_| ())?;
    let label = format!("local-attendee-{id}");
    if let Some(window) = app.get_webview_window(&label) {
        window.unminimize().map_err(|_| ())?;
        window.show().map_err(|_| ())?;
        return window.set_focus().map_err(|_| ());
    }
    let fragment = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("packet", &serde_json::to_string(&packet).map_err(|_| ())?)
        .finish();
    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?attendee-create={id}#{fragment}").into()),
    )
    .title(format!("{} — 내 PC에서 에이전트 추가", packet.room_id))
    .inner_size(720.0, 800.0)
    .min_inner_size(360.0, 480.0)
    .build()
    .map_err(|_| ())?;
    Ok(())
}

fn decode(url: &Url) -> Option<AttendeeEntryPacket> {
    if url.as_str().len() > 32768
        || url.scheme() != agentsassemble_domain::PROVIDER_SETUP_SCHEME
        || url.host_str() != Some("attend")
        || !url.path().is_empty()
        || url.query().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let fields: Vec<_> = url::form_urlencoded::parse(url.fragment()?.as_bytes()).collect();
    let [(key, value)] = fields.as_slice() else {
        return None;
    };
    if key != "packet" || value.len() > 8192 {
        return None;
    }
    let packet: AttendeeEntryPacket = serde_json::from_str(value).ok()?;
    for id in [&packet.request_id, &packet.room_uid, &packet.invite_id] {
        let parsed = Uuid::parse_str(id).ok()?;
        if parsed.is_nil() || parsed.to_string() != *id {
            return None;
        }
    }
    if agentsassemble_domain::validate_room_id(&packet.room_id).ok()? != packet.room_id
        || packet.display_name.trim().is_empty()
        || packet.display_name.chars().count() > 120
        || packet.display_name.chars().any(char::is_control)
        || packet.provider.is_empty()
        || packet.provider.len() > 64
        || !packet
            .provider
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        || packet.attend_command != format!("assemble room attend --provider {}", packet.provider)
        || packet.expires_at.is_empty()
        || packet.expires_at.len() > 64
    {
        return None;
    }
    let join = Url::parse(&packet.join_url).ok()?;
    let query: Vec<_> = join.query_pairs().collect();
    let [(name, token)] = query.as_slice() else {
        return None;
    };
    if join.scheme() != "https"
        || join.host_str().is_none()
        || join.path() != "/join"
        || !join.username().is_empty()
        || join.password().is_some()
        || join.fragment().is_some()
        || name != "token"
        || token.is_empty()
    {
        return None;
    }
    Some(packet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(value: &serde_json::Value) -> Url {
        let encoded = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("packet", &value.to_string())
            .finish();
        Url::parse(&format!("agentsassemble://attend#{encoded}"))
            .unwrap_or_else(|_| panic!("fixture handoff URL"))
    }

    #[test]
    fn handoff_is_a_bounded_invitation_draft_without_executable_or_human_authority() {
        let packet = serde_json::json!({
            "request_id":Uuid::new_v4().to_string(), "room_id":"general", "room_uid":Uuid::new_v4().to_string(),
            "invite_id":Uuid::new_v4().to_string(), "expires_at":"2026-09-10T00:00:00Z", "display_name":"My own AI",
            "provider":"codex", "attend_command":"assemble room attend --provider codex",
            "join_url":"https://room.example.test/join?token=controlled-one-use-invitation"
        });
        assert_eq!(
            decode(&link(&packet)).map(|value| value.display_name),
            Some("My own AI".to_owned())
        );
        for (key, value) in [
            ("command", "/bin/sh"),
            ("session_token", "human-credential"),
            ("workspace", "/private/workspace"),
        ] {
            let mut injected = packet.clone();
            injected[key] = value.into();
            assert!(decode(&link(&injected)).is_none());
        }
        for target in [
            "http://room.example.test/join?token=x",
            "https://user:secret@room.example.test/join?token=x",
            "https://room.example.test/join?token=x&token=y",
        ] {
            let mut invalid = packet.clone();
            invalid["join_url"] = target.into();
            assert!(decode(&link(&invalid)).is_none());
        }
        let mut wrong = link(&packet);
        wrong.set_query(Some("start=true"));
        assert!(decode(&wrong).is_none());
    }
}
