use deploy_platform::hostname::{preview_hostname, preview_label};

#[test]
fn preview_hostname_basic() {
    let hostname = preview_hostname("my-project", 42).expect("valid hostname");
    assert_eq!(hostname, "my-project-pr-42.preview.local");

    let label = preview_label("my-project", 42).expect("valid label");
    assert_eq!(label, "my-project-pr-42");
}

#[test]
fn preview_hostname_sanitizes_unsafe_and_unicode_characters() {
    let hostname = preview_hostname("My Project / Feature_1! 🚀", 7).expect("sanitized hostname");
    // Should be lowercased, unsafe chars and unicode converted to hyphens without doubles or leading/trailing
    assert_eq!(hostname, "my-project-feature-1-pr-7.preview.local");
}

#[test]
fn preview_hostname_rejects_empty_or_all_invalid() {
    assert!(preview_hostname("", 1).is_err());
    assert!(preview_hostname("   ", 2).is_err());
    assert!(preview_hostname("---", 3).is_err());
    assert!(preview_hostname("!!!///@@@", 4).is_err());
}

#[test]
fn preview_hostname_maximum_label_length() {
    let very_long_slug = "a".repeat(120);
    let hostname = preview_hostname(&very_long_slug, 1234).expect("truncated hostname");
    let label = preview_label(&very_long_slug, 1234).expect("truncated label");

    // RFC 1035 / RFC 1123: DNS label must be <= 63 characters
    assert!(label.len() <= 63);
    assert!(label.ends_with("-pr-1234"));
    assert_eq!(hostname, format!("{label}.preview.local"));
}

#[test]
fn preview_hostname_collision_resistance_on_truncation() {
    let base = "super-long-project-name-that-definitely-exceeds-the-maximum-dns-label-limit-";
    let slug_a = format!("{}variant-alpha", base);
    let slug_b = format!("{}variant-beta", base);

    let host_a = preview_hostname(&slug_a, 10).unwrap();
    let host_b = preview_hostname(&slug_b, 10).unwrap();

    assert_ne!(
        host_a, host_b,
        "different long slugs must produce distinct hostnames"
    );
}

#[test]
fn preview_hostname_deterministic() {
    let host1 = preview_hostname("analytics-dashboard", 99).unwrap();
    let host2 = preview_hostname("analytics-dashboard", 99).unwrap();
    assert_eq!(host1, host2);
}
