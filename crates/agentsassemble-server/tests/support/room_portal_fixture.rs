use std::path::Path;

pub(super) fn script(
    transcript: &Path,
    observed_views: &Path,
    turn_seen: &Path,
    release: &Path,
) -> String {
    format!(
        r#"#!/bin/sh
umask 077
portal_root=
for argument in "$@"
do
    case "$argument" in
        mcp_servers.agentsassemble_room.args=*)
            portal_root=${{argument#*\"--root\",\"}}
            portal_root=${{portal_root%\"\]*}}
            ;;
    esac
done
stage_room_outcome() {{
    content=$1
    test -n "$portal_root" || exit 40
    test -s "$portal_root/view.txt" || exit 41
    while IFS= read -r view_line || [ -n "$view_line" ]
    do
        printf '%s\n' "$view_line" >> {views}
    done < "$portal_root/view.txt"
    IFS= read -r authority < "$portal_root/turn.json"
    turn_id=${{authority#*\"turn_id\":\"}}
    turn_id=${{turn_id%%\"*}}
    input_seq=${{authority#*\"input_up_to_seq\":}}
    input_seq=${{input_seq%%,*}}
    test -n "$turn_id" || exit 42
    test -n "$input_seq" || exit 43
    printf '{{"turn_id":"%s","observed_through_seq":%s}}' "$turn_id" "$input_seq" > "$portal_root/receipt.json"
    printf '{{"kind":"message","turn_id":"%s","content":"%s","target_agent_id":""}}' "$turn_id" "$content" > "$portal_root/outcome.json"
}}
IFS= read -r initialize
printf '%s\n' "$initialize" >> {log}
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{}}}}'
IFS= read -r initialized
printf '%s\n' "$initialized" >> {log}
IFS= read -r thread
printf '%s\n' "$thread" >> {log}
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"thread":{{"id":"thread-1"}}}}}}'
IFS= read -r turn_one
printf '%s\n' "$turn_one" >> {log}
printf seen > {seen}
while [ ! -f {release} ]; do :; done
stage_room_outcome 'first room answer'
printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"provider-turn-1"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"agent_message/completed","params":{{"threadId":"thread-1","turnId":"provider-turn-1","text":"ignored first assistant final"}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread-1","turnId":"provider-turn-1"}}}}'
IFS= read -r turn_two
printf '%s\n' "$turn_two" >> {log}
stage_room_outcome 'second room answer'
printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{"turn":{{"id":"provider-turn-2"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"agent_message/completed","params":{{"threadId":"thread-1","turnId":"provider-turn-2","text":"ignored second assistant final"}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread-1","turnId":"provider-turn-2"}}}}'
IFS= read -r forever
"#,
        log = quote(transcript),
        views = quote(observed_views),
        seen = quote(turn_seen),
        release = quote(release),
    )
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
