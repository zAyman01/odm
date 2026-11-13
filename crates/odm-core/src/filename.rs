//! Safe filenames for downloaded files.

/// Makes a server- or user-supplied name safe to use as one file in a folder.
///
/// Returns `None` when nothing usable is left.
pub fn sanitize_filename(candidate: &str) -> Option<String> {
    let leaf = candidate.rsplit(['/', '\\']).next().unwrap_or(candidate);
    let cleaned: String = leaf
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    let trimmed = cleaned.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return None;
    }

    let mut name = if is_reserved_device_name(trimmed) {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    };
    if name.len() > MAX_FILENAME_BYTES {
        let mut end = MAX_FILENAME_BYTES;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name.truncate(end);
    }
    Some(name)
}

/// Longest name kept, in bytes; most filesystems allow 255.
const MAX_FILENAME_BYTES: usize = 200;

/// Windows treats these names as devices whatever the extension.
fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper.strip_prefix(prefix).is_some_and(|digit| {
                let mut chars = digit.chars();
                matches!((chars.next(), chars.next()), (Some('1'..='9'), None))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_last_path_segment() {
        assert_eq!(
            sanitize_filename("../../evil.exe").as_deref(),
            Some("evil.exe")
        );
        assert_eq!(sanitize_filename(r"C:\x\y.txt").as_deref(), Some("y.txt"));
    }

    #[test]
    fn removes_windows_forbidden_characters() {
        assert_eq!(
            sanitize_filename("image.jpg:large").as_deref(),
            Some("image.jpglarge")
        );
        assert_eq!(sanitize_filename("a<b>.txt").as_deref(), Some("ab.txt"));
        assert_eq!(
            sanitize_filename("a\"b|c?d*e.txt").as_deref(),
            Some("abcde.txt")
        );
        assert_eq!(
            sanitize_filename("tab\tname.txt").as_deref(),
            Some("tabname.txt")
        );
    }

    #[test]
    fn prefixes_reserved_device_names() {
        assert_eq!(sanitize_filename("con.txt").as_deref(), Some("_con.txt"));
        assert_eq!(sanitize_filename("NUL").as_deref(), Some("_NUL"));
        assert_eq!(
            sanitize_filename("Com1.tar.gz").as_deref(),
            Some("_Com1.tar.gz")
        );
        assert_eq!(sanitize_filename("lpt9").as_deref(), Some("_lpt9"));
        assert_eq!(
            sanitize_filename("console.txt").as_deref(),
            Some("console.txt")
        );
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename("name. ").as_deref(), Some("name"));
        assert_eq!(sanitize_filename("name.").as_deref(), Some("name"));
    }

    #[test]
    fn rejects_empty_and_dot_names() {
        assert_eq!(sanitize_filename(""), None);
        assert_eq!(sanitize_filename(".."), None);
        assert_eq!(sanitize_filename("."), None);
        assert_eq!(sanitize_filename("a/"), None);
        assert_eq!(sanitize_filename("???"), None);
    }

    #[test]
    fn truncates_long_names_on_a_char_boundary() {
        let long = "é".repeat(150);
        let cleaned = sanitize_filename(&long).unwrap();
        assert!(cleaned.len() <= 200, "len {}", cleaned.len());
        assert!(cleaned.chars().all(|c| c == 'é'));
    }
}
