use std::{fmt::Write, path::Path};

pub fn script(transcript: &Path, turn_seen: &Path, reject_interrupt: bool) -> String {
    format!(
        r#"#!/bin/sh
IFS= read -r initialize
printf '%s\n' "$initialize" >> {log}
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{}}}}'
IFS= read -r initialized
printf '%s\n' "$initialized" >> {log}
IFS= read -r thread
printf '%s\n' "$thread" >> {log}
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"thread":{{"id":"thread-1"}}}}}}'
IFS= read -r name
printf '%s\n' "$name" >> {log}
printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{}}}}'
request_id=4
IFS= read -r turn
while :; do
    case "$turn" in
        *'"method":"thread/unsubscribe"'*)
            printf '%s\n' "$turn" >> {log}
            printf '{{"id":%s,"result":{{"status":"unsubscribed"}}}}\n' "$request_id"
            ;;
        *'"method":"thread/resume"'*)
            printf '%s\n' "$turn" >> {log}
            printf '{{"id":%s,"result":{{"thread":{{"id":"thread-1"}}}}}}\n' "$request_id"
            ;;
        *) break ;;
    esac
    request_id=$((request_id + 1))
    IFS= read -r turn
done
printf '%s\n' "$turn" >> {log}
printf '{{"jsonrpc":"2.0","id":%s,"result":{{"turn":{{"id":"provider-turn-1"}}}}}}\n' "$request_id"
printf seen > {seen}
request_id=$((request_id + 1))
IFS= read -r interrupt
printf '%s\n' "$interrupt" >> {log}
if [ {reject} = 1 ]; then
    printf '{{"jsonrpc":"2.0","id":%s,"error":{{"code":-32000,"message":"fixture rejected interrupt"}}}}\n' "$request_id"
    IFS= read -r forever
    exit 0
fi
printf '{{"jsonrpc":"2.0","id":%s,"result":{{}}}}\n' "$request_id"
printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread-1","turn":{{"id":"provider-turn-1","status":"interrupted","items":[]}}}}}}'
IFS= read -r forever
"#,
        log = shell_quote(transcript),
        seen = shell_quote(turn_seen),
        reject = u8::from(reject_interrupt),
    )
}

fn shell_quote(path: &Path) -> String {
    let mut quoted = String::from("'");
    for character in path.to_string_lossy().chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted
                .write_char(character)
                .unwrap_or_else(|error| panic!("quote fixture path: {error}"));
        }
    }
    quoted.push('\'');
    quoted
}
