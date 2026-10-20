use semver::Version;

/// OpenCode 1 prints the bare version. OpenCode 2 prints `opencode v<version>`.
/// Both forms name the same semantic version, so callers never guess from a probe
/// when the installed CLI already reports it.
pub(crate) fn parse(output: &str) -> Option<Version> {
    let output = output.trim();
    let version = output
        .strip_prefix("opencode v")
        .or_else(|| output.strip_prefix("opencode "))
        .unwrap_or(output);
    Version::parse(version).ok()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn version_accepts_both_release_line_shapes() {
        assert_eq!(parse("2.0.32\n").map(|version| version.major), Some(2));
        assert_eq!(
            parse("opencode v2.0.32\n").map(|version| version.major),
            Some(2)
        );
        assert_eq!(parse("1.18.0").map(|version| version.major), Some(1));
        assert_eq!(
            parse("opencode 1.18.0").map(|version| version.major),
            Some(1)
        );
        assert!(parse("opencode latest").is_none());
        assert!(parse("private diagnostic").is_none());
    }
}
