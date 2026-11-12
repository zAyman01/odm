//! Site grabber core engine for crawling same-host web pages and extracting file assets.

use std::collections::{HashSet, VecDeque};

use url::Url;

/// Rule for robots.txt path matching.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RobotsRule {
    path_prefix: String,
    allowed: bool,
}

/// Parsed robots.txt rules for `odm` and wildcard `*`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RobotsTxt {
    odm_rules: Vec<RobotsRule>,
    wildcard_rules: Vec<RobotsRule>,
}

impl RobotsTxt {
    /// Parses robots.txt content into structured rules.
    pub fn parse(content: &str) -> Self {
        let mut odm_rules = Vec::new();
        let mut wildcard_rules = Vec::new();
        let mut current_agents: Vec<String> = Vec::new();
        let mut in_directives = false;

        for raw_line in content.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();

                if key.eq_ignore_ascii_case("user-agent") {
                    if in_directives {
                        current_agents.clear();
                        in_directives = false;
                    }
                    current_agents.push(value.to_ascii_lowercase());
                } else if key.eq_ignore_ascii_case("disallow") {
                    in_directives = true;
                    if !value.is_empty() {
                        let rule = RobotsRule {
                            path_prefix: value.to_string(),
                            allowed: false,
                        };
                        for agent in &current_agents {
                            if agent == "odm" {
                                odm_rules.push(rule.clone());
                            } else if agent == "*" {
                                wildcard_rules.push(rule.clone());
                            }
                        }
                    }
                } else if key.eq_ignore_ascii_case("allow") {
                    in_directives = true;
                    if !value.is_empty() {
                        let rule = RobotsRule {
                            path_prefix: value.to_string(),
                            allowed: true,
                        };
                        for agent in &current_agents {
                            if agent == "odm" {
                                odm_rules.push(rule.clone());
                            } else if agent == "*" {
                                wildcard_rules.push(rule.clone());
                            }
                        }
                    }
                }
            }
        }

        Self {
            odm_rules,
            wildcard_rules,
        }
    }

    /// Checks whether the specified path is allowed for a given user-agent.
    pub fn is_allowed(&self, user_agent: &str, path: &str) -> bool {
        let rules = if user_agent.eq_ignore_ascii_case("odm") && !self.odm_rules.is_empty() {
            &self.odm_rules
        } else {
            &self.wildcard_rules
        };

        if rules.is_empty() {
            return true;
        }

        let mut longest_match_len: usize = 0;
        let mut result = true;

        for rule in rules {
            if path.starts_with(&rule.path_prefix) {
                let prefix_len = rule.path_prefix.len();
                if prefix_len > longest_match_len {
                    longest_match_len = prefix_len;
                    result = rule.allowed;
                } else if prefix_len == longest_match_len && rule.allowed {
                    result = true;
                }
            }
        }

        result
    }
}

/// Configuration parameters for a site grabber session.
#[derive(Debug, Clone)]
pub struct GrabberConfig {
    pub start_url: Url,
    pub depth_limit: u32,
    pub extensions: Vec<String>,
    pub pattern: Option<String>,
    pub respect_robots_txt: bool,
    pub max_pages: usize,
}

impl GrabberConfig {
    /// Creates a new grabber configuration with default settings.
    pub fn new(start_url: Url) -> Self {
        Self {
            start_url,
            depth_limit: 1,
            extensions: Vec::new(),
            pattern: None,
            respect_robots_txt: true,
            max_pages: 50,
        }
    }
}

/// A file asset discovered by the site grabber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrabbedFile {
    pub url: Url,
    pub filename: String,
    pub found_on: Url,
    pub depth: u32,
}

/// Result returned after crawling finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrabResult {
    pub files: Vec<GrabbedFile>,
    pub pages_crawled: usize,
}

/// Errors returned by the site grabber.
#[derive(Debug, thiserror::Error)]
pub enum GrabberError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("invalid selector: {0}")]
    Selector(String),
}

/// Crawls web pages on the same host and collects matching file downloads.
pub struct SiteGrabber {
    config: GrabberConfig,
}

impl SiteGrabber {
    /// Creates a new site grabber with the given configuration.
    pub fn new(config: GrabberConfig) -> Self {
        Self { config }
    }

    /// Crawls pages starting from `start_url` up to `depth_limit`.
    pub async fn crawl(&self, client: &reqwest::Client) -> Result<GrabResult, GrabberError> {
        let robots = if self.config.respect_robots_txt {
            let mut robots_url = self.config.start_url.clone();
            robots_url.set_path("/robots.txt");
            robots_url.set_query(None);
            robots_url.set_fragment(None);

            match client
                .get(robots_url)
                .header(reqwest::header::USER_AGENT, "odm/0.1.0")
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => match resp.text().await {
                    Ok(text) => RobotsTxt::parse(&text),
                    Err(_) => RobotsTxt::default(),
                },
                _ => RobotsTxt::default(),
            }
        } else {
            RobotsTxt::default()
        };

        if self.config.respect_robots_txt && !robots.is_allowed("odm", self.config.start_url.path())
        {
            return Ok(GrabResult {
                files: Vec::new(),
                pages_crawled: 0,
            });
        }

        let selector = scraper::Selector::parse(
            "a[href], img[src], source[src], video[src], audio[src], link[href]",
        )
        .map_err(|e| GrabberError::Selector(format!("{e:?}")))?;

        let mut visited_pages: HashSet<Url> = HashSet::new();
        let mut enqueued_pages: HashSet<Url> = HashSet::new();
        let mut queue: VecDeque<(Url, u32)> = VecDeque::new();

        let mut discovered_files: Vec<GrabbedFile> = Vec::new();
        let mut seen_file_urls: HashSet<Url> = HashSet::new();

        queue.push_back((self.config.start_url.clone(), 0));
        enqueued_pages.insert(self.config.start_url.clone());

        while let Some((current_url, current_depth)) = queue.pop_front() {
            if visited_pages.contains(&current_url) {
                continue;
            }
            if visited_pages.len() >= self.config.max_pages {
                break;
            }

            visited_pages.insert(current_url.clone());

            let resp = match client
                .get(current_url.clone())
                .header(reqwest::header::USER_AGENT, "odm/0.1.0")
                .send()
                .await
            {
                Ok(r) => r,
                Err(_) => continue,
            };

            if !resp.status().is_success() {
                continue;
            }

            if let Some(ct) = resp.headers().get(reqwest::header::CONTENT_TYPE)
                && let Ok(ct_str) = ct.to_str()
            {
                let ct_lower = ct_str.to_ascii_lowercase();
                if !ct_lower.contains("text/html") && !ct_lower.contains("application/xhtml+xml") {
                    continue;
                }
            }

            let html_content = match resp.text().await {
                Ok(t) => t,
                Err(_) => continue,
            };

            let document = scraper::Html::parse_document(&html_content);

            for element in document.select(&selector) {
                let val = element.value();
                let tag_name = val.name();
                let attr_val = match tag_name {
                    "a" | "link" => val.attr("href"),
                    "img" | "source" | "video" | "audio" => val.attr("src"),
                    _ => None,
                };

                let raw_val = match attr_val {
                    Some(v) => v.trim(),
                    None => continue,
                };

                if raw_val.is_empty() {
                    continue;
                }

                let mut target_url = match current_url.join(raw_val) {
                    Ok(u) => u,
                    Err(_) => continue,
                };
                target_url.set_fragment(None);

                if target_url.scheme() != "http" && target_url.scheme() != "https" {
                    continue;
                }

                // Same-host restriction
                if target_url.host_str() != self.config.start_url.host_str() {
                    continue;
                }

                // Robots.txt restriction
                if self.config.respect_robots_txt && !robots.is_allowed("odm", target_url.path()) {
                    continue;
                }

                let filename = filename_from_url(&target_url);

                // Check file criteria (extension and pattern)
                let matches_ext = if self.config.extensions.is_empty() {
                    true
                } else if let Some(ext) = get_extension(&target_url) {
                    self.config
                        .extensions
                        .iter()
                        .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext))
                } else {
                    false
                };

                let matches_pat = match &self.config.pattern {
                    Some(pat) => {
                        matches_pattern(pat, target_url.as_str()) || matches_pattern(pat, &filename)
                    }
                    None => true,
                };

                if matches_ext && matches_pat && seen_file_urls.insert(target_url.clone()) {
                    discovered_files.push(GrabbedFile {
                        url: target_url.clone(),
                        filename,
                        found_on: current_url.clone(),
                        depth: current_depth + 1,
                    });
                }

                // Consider enqueuing for deeper crawling if it is a hyperlink
                if tag_name == "a" && current_depth + 1 < self.config.depth_limit {
                    if let Some(ext) = get_extension(&target_url)
                        && is_non_crawlable_extension(&ext)
                    {
                        continue;
                    }

                    if !visited_pages.contains(&target_url)
                        && enqueued_pages.insert(target_url.clone())
                    {
                        queue.push_back((target_url, current_depth + 1));
                    }
                }
            }
        }

        Ok(GrabResult {
            files: discovered_files,
            pages_crawled: visited_pages.len(),
        })
    }
}

fn filename_from_url(url: &Url) -> String {
    if let Some(segments) = url.path_segments() {
        let non_empty: Vec<&str> = segments.filter(|s| !s.is_empty()).collect();
        if let Some(last) = non_empty.last() {
            if let Ok(decoded) = percent_encoding::percent_decode_str(last).decode_utf8() {
                return decoded.to_string();
            }
            return (*last).to_string();
        }
    }
    "download".to_string()
}

fn get_extension(url: &Url) -> Option<String> {
    let path = url.path();
    let filename = path.rsplit('/').next()?;
    let ext = filename.rsplit('.').next()?;
    if ext == filename || ext.is_empty() {
        None
    } else {
        Some(ext.to_ascii_lowercase())
    }
}

fn is_non_crawlable_extension(ext: &str) -> bool {
    matches!(
        ext,
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "svg"
            | "ico"
            | "bmp"
            | "zip"
            | "tar"
            | "gz"
            | "7z"
            | "rar"
            | "pdf"
            | "mp3"
            | "mp4"
            | "mkv"
            | "avi"
            | "wav"
            | "flac"
            | "ogg"
            | "exe"
            | "dmg"
            | "iso"
            | "bin"
            | "apk"
            | "deb"
            | "rpm"
            | "msi"
            | "css"
            | "js"
            | "json"
            | "xml"
            | "woff"
            | "woff2"
            | "ttf"
            | "eot"
    )
}

/// Matches a wildcard pattern containing `*` and `?` or substring against target text.
pub fn matches_pattern(pattern: &str, text: &str) -> bool {
    let mut normalized = pattern;
    if let Some(stripped) = normalized.strip_prefix('^') {
        normalized = stripped;
    }
    if let Some(stripped) = normalized.strip_suffix('$') {
        normalized = stripped;
    }
    let converted = normalized.replace(".*", "*").replace("\\.", ".");

    if !converted.contains('*') && !converted.contains('?') {
        return text.contains(&converted);
    }

    let p: Vec<char> = converted.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    while t_idx < t.len() {
        if p_idx < p.len() && (p[p_idx] == '?' || p[p_idx] == t[t_idx]) {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p.len() && p[p_idx] == '*' {
            star_idx = Some(p_idx);
            match_idx = t_idx;
            p_idx += 1;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < p.len() && p[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_robots_txt_parsing_and_matching() {
        let content = r#"
# Robots.txt comment
User-agent: odm
Disallow: /private/
Allow: /private/public/

User-agent: *
Disallow: /secret/
"#;
        let robots = RobotsTxt::parse(content);

        // odm user agent matches odm section
        assert!(!robots.is_allowed("odm", "/private/file.txt"));
        assert!(robots.is_allowed("odm", "/private/public/data.png"));
        assert!(robots.is_allowed("odm", "/secret/other.zip"));

        // other user agents match wildcard section
        assert!(robots.is_allowed("googlebot", "/private/file.txt"));
        assert!(!robots.is_allowed("googlebot", "/secret/file.zip"));
        assert!(robots.is_allowed("googlebot", "/public/index.html"));
    }

    #[test]
    fn test_pattern_matching() {
        assert!(matches_pattern("*.pdf", "http://example.com/doc.pdf"));
        assert!(!matches_pattern("*.pdf", "http://example.com/doc.png"));
        assert!(matches_pattern(
            "*report*",
            "http://example.com/report_2026.pdf"
        ));
        assert!(matches_pattern(
            ".*report.*",
            "http://example.com/report_2026.pdf"
        ));
        assert!(matches_pattern(
            "report",
            "http://example.com/report_2026.pdf"
        ));
    }
}
