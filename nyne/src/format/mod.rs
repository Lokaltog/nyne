//! Shared formatting utilities.

/// Format a git timestamp (seconds since epoch) as `YYYY-MM-DD`.
use jiff::tz::TimeZone;

pub fn format_git_date(epoch_secs: i64) -> String {
    jiff::Timestamp::from_second(epoch_secs).map_or_else(
        |_| "-".to_owned(),
        |ts| ts.to_zoned(TimeZone::UTC).strftime("%Y-%m-%d").to_string(),
    )
}

/// Convert a string to kebab-case suitable for filenames, truncated to
/// `max_len` at a hyphen boundary.
pub fn to_kebab(s: &str, max_len: usize) -> String { truncate_kebab(&to_kebab_raw(s), max_len) }

/// Convert a string to kebab-case, stripping non-alphanumeric characters
/// before applying case conversion.
pub fn to_kebab_raw(s: &str) -> String {
    use convert_case::{Case, Casing};
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_case(Case::Kebab)
}

/// Truncate a kebab-case string to `max_len`, cutting at a hyphen boundary.
fn truncate_kebab(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_owned();
    }
    let slice = &s[..max_len];
    slice
        .rfind('-')
        .map_or_else(|| slice.to_owned(), |pos| slice[..pos].to_owned())
}

/// Compute a unified diff between two strings using `similar`.
///
/// Produces standard `a/`/`b/`-prefixed headers. Returns an empty string
/// when `old` and `new` are identical.
pub fn unified_diff(old: &str, new: &str, path: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}

#[cfg(test)]
mod tests;
