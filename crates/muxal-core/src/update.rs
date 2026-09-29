//! Pure logic behind the settings "Check for updates": parsing the GitHub
//! releases payload and comparing versions. No I/O lives here.

/// The GitHub API endpoint the in-app check asks (GET, click-only).
pub const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/bencordeiro/muxal/releases/latest";

/// What the update check found relative to the running version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// No check has run yet.
    Idle,
    /// A check is in flight.
    Checking,
    /// The latest release is this very version.
    UpToDate,
    /// A newer release exists (`latest` is its `v`-prefixed tag).
    Available { latest: String },
    /// The check could not complete (offline, rate-limited, changed payload).
    Failed,
}

/// Extract the release tag from a GitHub `releases/latest` JSON payload.
pub fn latest_release_tag(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag = value.get("tag_name")?.as_str()?.trim().to_string();
    (!tag.is_empty()).then_some(tag)
}

/// Whether `latest` names a version newer than `current`. Accepts a leading
/// `v`; `major.minor.patch` with missing parts counting as 0; a pre-release of
/// the same triple counts as older than the release.
pub fn version_is_newer(latest: &str, current: &str) -> bool {
    parse_version(latest) > parse_version(current)
}

/// Fold a freshly fetched tag into the check outcome for `current`. `None`
/// (the fetch failed) becomes [`UpdateState::Failed`].
pub fn update_state_for(current: &str, latest_tag: Option<&str>) -> UpdateState {
    match latest_tag {
        Some(tag) if version_is_newer(tag, current) => UpdateState::Available {
            latest: tag.to_string(),
        },
        Some(_) => UpdateState::UpToDate,
        None => UpdateState::Failed,
    }
}

/// `(major, minor, patch, is_release)` — the flag inverted so a release sorts
/// above a pre-release of the same triple. Pre-release *ordering* (rc1 vs rc2)
/// is deliberately ignored: muxal releases are plain triples.
fn parse_version(v: &str) -> (u64, u64, u64, bool) {
    let core = v.trim().trim_start_matches(['v', 'V']);
    let (numbers, pre) = match core.split_once('-') {
        Some((n, _)) => (n, true),
        None => (core, false),
    };
    let mut parts = numbers
        .split('.')
        .map(|p| p.trim().parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        !pre,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_handles_v_prefixes_and_placeholders() {
        assert!(version_is_newer("v0.4.4", "0.4.3"));
        assert!(version_is_newer("0.4.10", "0.4.3"));
        assert!(version_is_newer("v0.5.0", "0.4.9"));
        assert!(!version_is_newer("v0.4.3", "0.4.3"));
        assert!(!version_is_newer("v0.4.2", "0.4.3"));
        // Missing parts count as zero; a pre-release is older than its release.
        assert!(version_is_newer("v1.0", "0.9.9"));
        assert!(!version_is_newer("v1.0.0-rc1", "1.0.0"));
    }

    #[test]
    fn latest_release_tag_reads_tag_name_only() {
        assert_eq!(
            latest_release_tag(r#"{"tag_name":"v0.4.4","name":"v0.4.4"}"#).as_deref(),
            Some("v0.4.4")
        );
        assert_eq!(latest_release_tag(r#"{"message":"Not Found"}"#), None);
        assert_eq!(latest_release_tag("not json"), None);
    }

    #[test]
    fn update_states_derive_from_the_fetched_tag() {
        assert_eq!(
            update_state_for("0.4.3", Some("v0.4.4")),
            UpdateState::Available {
                latest: "v0.4.4".into()
            }
        );
        assert_eq!(
            update_state_for("0.4.3", Some("v0.4.3")),
            UpdateState::UpToDate
        );
        assert_eq!(update_state_for("0.4.3", None), UpdateState::Failed);
    }
}
