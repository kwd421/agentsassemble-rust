//! Shared policy for diagnostic text that crosses a durable or public boundary.

use std::sync::OnceLock;

use regex::{Captures, Regex};

const DEFAULT_DIAGNOSTIC_LIMIT: usize = 16_000;

fn regex(pattern: &'static str, slot: &'static OnceLock<Regex>) -> &'static Regex {
    slot.get_or_init(|| {
        Regex::new(pattern)
            .unwrap_or_else(|error| panic!("invalid diagnostic redaction regex: {error}"))
    })
}

fn sensitive_http_header() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r"(?im)(?P<prefix>^|[\s;,])(?P<name>authorization|proxy-authorization|cookie|set-cookie|x-api-key|x-auth-token)\s*:\s*[^\r\n]*",
        &VALUE,
    )
}

fn pem_private_key() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r"(?is)-----BEGIN\s+(?:[A-Z0-9]+\s+)*PRIVATE\s+KEY-----.*?(?:-----END\s+(?:[A-Z0-9]+\s+)*PRIVATE\s+KEY-----|\z)",
        &VALUE,
    )
}

fn jwt_value() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r"(?P<prefix>^|[^A-Za-z0-9_-])eyJ[A-Za-z0-9_-]{8,}(?:\.[A-Za-z0-9_-]{8,}){1,4}(?P<suffix>[^A-Za-z0-9_-]|$)",
        &VALUE,
    )
}

fn bearer_value() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(r"(?i)\bbearer\s+\S+", &VALUE)
}

fn sensitive_assignment() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r#"(?ix)(?P<prefix>^|[^a-z0-9_])(?:["'])?(?:authorization|auth|api[_-]?key|access[_-]?token|refresh[_-]?token|password|passwd|credential|secret|token)(?:["'])?\s*(?:=|:)\s*(?:"[^"]*"|'[^']*'|`[^`]*`|[^\s,;]+)"#,
        &VALUE,
    )
}

fn sensitive_option() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r#"(?ix)--?(?:authorization|auth|api[_-]?key|access[_-]?token|refresh[_-]?token|password|passwd|credential|secret|token)(?:=|\s+)(?:"[^"]*"|'[^']*'|`[^`]*`|[^\s,;]+)"#,
        &VALUE,
    )
}

fn basic_auth_option() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r#"(?ix)(?P<prefix>^|\s)-u\s+(?:"[^"]*"|'[^']*'|[^\s,;]+)"#,
        &VALUE,
    )
}

fn url_userinfo() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r"(?i)\b(?P<scheme>[a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@",
        &VALUE,
    )
}

fn secret_prefix() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r"(?ix)\b(?:(?:sk|aai1|ghp|github_pat|llmgtwy|vck|csk|hf|glpat|npm|dop_v1)[-_.][A-Za-z0-9._-]{6,}|AIza[A-Za-z0-9_-]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16}|xox[baprs]-[A-Za-z0-9-]{10,})\b",
        &VALUE,
    )
}

fn windows_path() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r#"(?ix)(?P<prefix>^|[\s'"`=(])(?:[a-z]:[\\/]|\\\\)[^\s'"`|;&<>]*"#,
        &VALUE,
    )
}

fn home_path() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r#"(?x)(?P<prefix>^|[\s'"`=(])~(?:/[^\s'"`|;&<>]*)?"#,
        &VALUE,
    )
}

fn unix_path() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    regex(
        r#"(?x)(?P<prefix>^|[\s'"`=(])/(?:[^/\s'"`|;&<>][^\s'"`|;&<>]*)?"#,
        &VALUE,
    )
}

fn replace_preserving(text: &str, matcher: &Regex, replacement: &str) -> String {
    matcher
        .replace_all(text, |captures: &Captures<'_>| {
            format!(
                "{}{}{}",
                captures.name("prefix").map_or("", |value| value.as_str()),
                replacement,
                captures.name("suffix").map_or("", |value| value.as_str())
            )
        })
        .into_owned()
}

fn tail_chars(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        text.to_owned()
    } else {
        text.chars().skip(count - limit).collect()
    }
}

/// Removes credentials and local paths before diagnostic text becomes durable.
///
/// The tail is retained because it normally contains the useful process error.
#[must_use]
pub fn redact_persisted_diagnostic_text(value: &str, limit: usize) -> String {
    let bounded_limit = limit.max(1);
    let mut text = value.replace('\0', "").trim().to_owned();
    if text.is_empty() {
        return text;
    }

    text = pem_private_key()
        .replace_all(&text, "[redacted private key]")
        .into_owned();
    text = sensitive_http_header()
        .replace_all(&text, "${prefix}${name}: [redacted]")
        .into_owned();
    text = replace_preserving(&text, jwt_value(), "[redacted JWT]");
    text = tail_chars(&text, bounded_limit.saturating_mul(2).max(32_000));
    text = bearer_value()
        .replace_all(&text, "Bearer [redacted]")
        .into_owned();
    text = replace_preserving(&text, sensitive_assignment(), "[redacted]");
    text = sensitive_option()
        .replace_all(&text, "[redacted]")
        .into_owned();
    text = replace_preserving(&text, basic_auth_option(), "-u [redacted]");
    text = url_userinfo()
        .replace_all(&text, "${scheme}[redacted]@")
        .into_owned();
    text = secret_prefix()
        .replace_all(&text, "[redacted]")
        .into_owned();
    text = replace_preserving(&text, windows_path(), "[local path]");
    text = replace_preserving(&text, home_path(), "[local path]");
    text = replace_preserving(&text, unix_path(), "[local path]");
    tail_chars(&text, bounded_limit).trim().to_owned()
}

/// Uses the original application's default durable diagnostic bound.
#[must_use]
pub fn redact_persisted_diagnostic(value: &str) -> String {
    redact_persisted_diagnostic_text(value, DEFAULT_DIAGNOSTIC_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::redact_persisted_diagnostic_text;

    #[test]
    fn removes_provider_credentials_and_local_paths() {
        let input = concat!(
            "/Users/alice/private/bin/codex:\n",
            "Authorization: Bearer extremely-private-token\n",
            "--api-key=sk-live-example123456\n",
            "https://operator:password@example.test/v1\n",
            "C:\\Users\\alice\\secret.env"
        );
        let redacted = redact_persisted_diagnostic_text(input, 4_000);

        assert!(!redacted.contains("alice"));
        assert!(!redacted.contains("extremely-private-token"));
        assert!(!redacted.contains("sk-live-example123456"));
        assert!(!redacted.contains("operator:password"));
        assert!(redacted.contains("[redacted]"));
        assert!(redacted.contains("[local path]"));
    }

    #[test]
    fn strips_nul_and_keeps_the_bounded_tail() {
        let redacted = redact_persisted_diagnostic_text("old-prefix\0useful-tail", 11);
        assert_eq!(redacted, "useful-tail");
    }
}
