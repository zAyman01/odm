//! Batch URL expansion using bracket syntax.

/// Errors occurring during batch URL bracket parsing and expansion.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BatchError {
    #[error("unclosed bracket in batch pattern")]
    UnclosedBracket,
    #[error("invalid step in batch pattern")]
    InvalidStep,
    #[error("invalid range specification in batch pattern: {0}")]
    InvalidRange(String),
}

/// Expands a batch URL containing bracketed ranges into a vector of concrete URLs.
///
/// Supported syntax:
/// - Numerical ranges: `[001-100]` with preserved zero-padding
/// - Stepped numerical ranges: `[1-10:2]`
/// - Character ranges: `[a-z]`, `[A-Z]`
/// - Multiple brackets (Cartesian product): `[01-02]_[a-b]`
pub fn expand_batch_url(pattern: &str) -> Result<Vec<String>, BatchError> {
    let mut parts: Vec<Vec<String>> = Vec::new();
    let mut chars = pattern.char_indices().peekable();
    let mut last_idx = 0;

    while let Some((idx, ch)) = chars.next() {
        if ch == '[' {
            if idx > last_idx {
                parts.push(vec![pattern[last_idx..idx].to_string()]);
            }
            let mut close_idx = None;
            for (next_idx, next_ch) in chars.by_ref() {
                if next_ch == ']' {
                    close_idx = Some(next_idx);
                    break;
                }
            }
            let close = close_idx.ok_or(BatchError::UnclosedBracket)?;
            let content = &pattern[idx + 1..close];
            let expanded = parse_bracket_range(content)?;
            parts.push(expanded);
            last_idx = close + 1;
        }
    }

    if last_idx < pattern.len() {
        parts.push(vec![pattern[last_idx..].to_string()]);
    }

    if parts.is_empty() {
        return Ok(vec![pattern.to_string()]);
    }

    let mut current = vec![String::new()];
    for part in parts {
        let mut next = Vec::with_capacity(current.len() * part.len());
        for prefix in &current {
            for item in &part {
                let mut s = String::with_capacity(prefix.len() + item.len());
                s.push_str(prefix);
                s.push_str(item);
                next.push(s);
            }
        }
        current = next;
    }

    Ok(current)
}

fn parse_bracket_range(content: &str) -> Result<Vec<String>, BatchError> {
    let (range_str, step_str) = match content.split_once(':') {
        Some((r, s)) => (r, Some(s)),
        None => (content, None),
    };

    let step = if let Some(s) = step_str {
        let st = s.parse::<usize>().map_err(|_| BatchError::InvalidStep)?;
        if st == 0 {
            return Err(BatchError::InvalidStep);
        }
        st
    } else {
        1
    };

    let (start_str, end_str) = range_str
        .split_once('-')
        .ok_or_else(|| BatchError::InvalidRange(content.to_string()))?;

    if start_str.is_empty() || end_str.is_empty() {
        return Err(BatchError::InvalidRange(content.to_string()));
    }

    // Check alphabetic
    if start_str.len() == 1 && end_str.len() == 1 {
        let c1 = match start_str.chars().next() {
            Some(c) => c,
            None => return Err(BatchError::InvalidRange(content.to_string())),
        };
        let c2 = match end_str.chars().next() {
            Some(c) => c,
            None => return Err(BatchError::InvalidRange(content.to_string())),
        };

        if c1.is_ascii_alphabetic() && c2.is_ascii_alphabetic() {
            if (c1.is_ascii_lowercase() && c2.is_ascii_lowercase())
                || (c1.is_ascii_uppercase() && c2.is_ascii_uppercase())
            {
                let mut res = Vec::new();
                if c1 <= c2 {
                    let mut curr = c1 as u8;
                    let target = c2 as u8;
                    while curr <= target {
                        res.push((curr as char).to_string());
                        if (curr as usize) + step > (target as usize) {
                            break;
                        }
                        curr += step as u8;
                    }
                } else {
                    let mut curr = c1 as u8;
                    let target = c2 as u8;
                    while curr >= target {
                        res.push((curr as char).to_string());
                        if (curr as usize) < (target as usize) + step {
                            break;
                        }
                        curr -= step as u8;
                    }
                }
                return Ok(res);
            } else {
                return Err(BatchError::InvalidRange(content.to_string()));
            }
        }
    }

    // Check numeric
    let start_num = start_str
        .parse::<u64>()
        .map_err(|_| BatchError::InvalidRange(content.to_string()))?;
    let end_num = end_str
        .parse::<u64>()
        .map_err(|_| BatchError::InvalidRange(content.to_string()))?;

    let width = if start_str.starts_with('0') || end_str.starts_with('0') {
        start_str.len().max(end_str.len())
    } else {
        0
    };

    let mut res = Vec::new();
    if start_num <= end_num {
        let mut curr = start_num;
        while curr <= end_num {
            if width > 0 {
                res.push(format!("{curr:0>width$}"));
            } else {
                res.push(curr.to_string());
            }
            if curr.checked_add(step as u64).is_none() || curr + (step as u64) > end_num {
                break;
            }
            curr += step as u64;
        }
    } else {
        let mut curr = start_num;
        while curr >= end_num {
            if width > 0 {
                res.push(format!("{curr:0>width$}"));
            } else {
                res.push(curr.to_string());
            }
            if curr < end_num + (step as u64) {
                break;
            }
            curr -= step as u64;
        }
    }

    Ok(res)
}
