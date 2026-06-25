use semver::Version;

fn is_semver_tag(tag: &str) -> bool {
    let Some(tag) = tag.strip_prefix('v') else {
        return false;
    };

    Version::parse(tag).is_ok()
}

fn normalize_release_tag(version: &str) -> Option<String> {
    let version = version.trim();
    let tag = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    };

    if is_semver_tag(&tag) {
        Some(tag)
    } else {
        None
    }
}

#[test]
fn semver_tag_accepts_expected_values() {
    for tag in [
        "v0.1.0",
        "v1.2.3",
        "v1.2.3-beta.1",
        "v1.2.3+build.7",
        "v1.2.3-beta.1+build.7",
    ] {
        assert!(is_semver_tag(tag), "expected valid tag to pass: {tag}");
    }
}

#[test]
fn semver_tag_rejects_invalid_values() {
    for tag in ["v", "v1", "v1.2", "1.2.3", "v01.2.3", "v1.02.3", "v1.2.03", "v1.2.3-", "v1.2.3+"] {
        assert!(!is_semver_tag(tag), "expected invalid tag to fail: {tag}");
    }
}

#[test]
fn normalize_release_tag_prefixes_and_validates() {
    assert_eq!(normalize_release_tag("1.2.3"), Some("v1.2.3".to_string()));
    assert_eq!(
        normalize_release_tag("v1.2.3-beta.1"),
        Some("v1.2.3-beta.1".to_string())
    );
    assert_eq!(normalize_release_tag("v"), None);
    assert_eq!(normalize_release_tag("v1"), None);
}
