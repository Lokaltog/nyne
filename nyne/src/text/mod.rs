//! Shared formatting utilities.

/// Format a git timestamp (seconds since epoch) as `YYYY-MM-DD`.
use jiff::tz::TimeZone;

/// Formats a Unix epoch timestamp as a `YYYY-MM-DD` date string in UTC.
pub fn format_git_date(epoch_secs: i64) -> String {
    jiff::Timestamp::from_second(epoch_secs).map_or_else(
        |_| "-".to_owned(),
        |ts| ts.to_zoned(TimeZone::UTC).strftime("%Y-%m-%d").to_string(),
    )
}

/// Produce a filesystem-safe slug, truncated to `max_len` at a hyphen
/// boundary.
pub fn slugify(s: &str, max_len: usize) -> String { truncate_at_boundary(&slugify_unbounded(s), max_len) }

/// Produce a filesystem-safe slug with no length limit.
///
/// Strips non-alphanumeric characters and delegates to `convert_case` for
/// kebab-case conversion.
pub fn slugify_unbounded(s: &str) -> String {
    use convert_case::{Case, Casing};
    // Split on non-alphanumeric boundaries directly, filtering empties to
    // normalize leading/trailing/consecutive separators. Avoids the extra
    // `chars().map().collect::<String>()` from splitting on the input directly.
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .fold(String::new(), |mut acc, part| {
            if !acc.is_empty() {
                acc.push(' ');
            }
            acc.push_str(part);
            acc
        })
        .to_case(Case::Kebab)
}

/// Truncate a slug to `max_len`, cutting at a hyphen boundary.
fn truncate_at_boundary(s: &str, max_len: usize) -> String {
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
/// Produces standard `a/`/`b/`-prefixed headers compatible with `patch -p1`.
/// Returns an empty string when `old` and `new` are identical.
///
/// `path` should be project-relative (e.g. `src/lib.rs`). The diff rendering
/// pipeline normalizes absolute paths via `strip_root_prefix` before calling
/// this function. A leading `/` is stripped defensively as a fallback.
pub fn unified_diff(old: &str, new: &str, path: &str) -> String {
    let path = path.strip_prefix('/').unwrap_or(path);
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}

/// Unit tests.
#[cfg(test)]
mod tests;

/// Estimate tokens from a byte count and format in compact form.
///
/// Converts bytes → tokens (`bytes / 4`) then formats (e.g. `~2.1k` for
/// 8400 bytes, `~850` for 3400 bytes). Registered as the `tokens`
/// minijinja filter by [`crate::templates::TemplateEngine`].
pub fn format_tokens(bytes: usize) -> String {
    let n = bytes / 4;
    if n >= 1000 {
        let whole = n / 1000;
        let frac = (n % 1000) / 100;
        format!("~{whole}.{frac}k")
    } else {
        format!("~{n}")
    }
}

/// Extract the first non-empty trimmed line from a string.
///
/// Registered as the `first_line` minijinja filter by
/// [`crate::templates::TemplateEngine`].
///
/// Returns `String` because minijinja's `Function` trait requires `Rv: FunctionResult`
/// with no input lifetime threading (`Args: for<'a> FunctionArgs<'a>`), so the return
/// type cannot borrow from the input `&str`. `Cow<str>` would still allocate here.
pub fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_owned()
}

/// Strip a prefix from a string, returning the original if no match.
///
/// Registered as the `strip_prefix` minijinja filter by
/// [`crate::templates::TemplateEngine`].
pub fn strip_prefix(mut v: String, prefix: &str) -> String {
    if v.starts_with(prefix) {
        v.replace_range(..prefix.len(), "");
    }
    v
}
