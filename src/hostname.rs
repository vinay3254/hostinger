use anyhow::{bail, Result};
use sha2::{Digest, Sha256};

/// Normalizes a project slug and PR number into a safe DNS label of at most 63 characters.
pub fn preview_label(project_slug: &str, pr_number: u64) -> Result<String> {
    let suffix = format!("-pr-{}", pr_number);

    // 1. Lowercase and replace non-alphanumeric ASCII characters with hyphens
    let mut sanitized = String::with_capacity(project_slug.len());
    let mut prev_hyphen = false;

    for ch in project_slug.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            sanitized.push('-');
            prev_hyphen = true;
        }
    }

    // Trim leading and trailing hyphens
    let trimmed = sanitized.trim_matches('-');

    if trimmed.is_empty() {
        bail!("invalid project slug: slug cannot be empty after normalization");
    }

    let max_label_len = 63;
    if trimmed.len() + suffix.len() <= max_label_len {
        return Ok(format!("{}{}", trimmed, suffix));
    }

    // If truncated, calculate a 6-char hex hash from the original project_slug to prevent collisions
    let mut hasher = Sha256::new();
    hasher.update(project_slug.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let hash_suffix = format!("-{}", &hash[..6]);

    // Format: <truncated_slug>-<hash>-pr-<pr_number>
    // Length: truncated_len + hash_suffix.len() (7) + suffix.len() <= 63
    let overhead = hash_suffix.len() + suffix.len();
    if overhead >= max_label_len {
        bail!("PR number or suffix is too large to fit in a 63-character DNS label");
    }

    let allowed_slug_len = max_label_len - overhead;
    let mut truncated = &trimmed[..allowed_slug_len.min(trimmed.len())];
    truncated = truncated.trim_end_matches('-');

    let label = format!("{}{}{}", truncated, hash_suffix, suffix);
    Ok(label)
}

/// Generates a preview hostname for a project slug and PR number.
pub fn preview_hostname(project_slug: &str, pr_number: u64) -> Result<String> {
    let label = preview_label(project_slug, pr_number)?;
    Ok(format!("{}.preview.local", label))
}
