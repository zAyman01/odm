//! Metalink (v3 and v4 XML) document parser.

use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;

use crate::checksum::{Checksum, ChecksumAlgorithm};

/// Information extracted from a Metalink document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetalinkSpec {
    pub filename: String,
    pub size: Option<u64>,
    pub urls: Vec<String>,
    pub checksum: Option<Checksum>,
}

/// Errors occurring during Metalink XML parsing.
#[derive(Debug, thiserror::Error)]
pub enum MetalinkError {
    #[error("xml parse error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("invalid metalink format: {0}")]
    Invalid(String),
}

/// Parses a Metalink v3 (`.metalink`) or Metalink v4 (`.meta4`) XML document.
pub fn parse_metalink(xml: &str) -> Result<MetalinkSpec, MetalinkError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut is_metalink = false;
    let mut filename = String::new();
    let mut size = None;
    let mut checksum = None;
    let mut urls_with_prio: Vec<(String, i64, usize)> = Vec::new();

    let mut current_tag = Vec::new();
    let mut current_hash_type = String::new();
    let mut current_url_prio = 0i64;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) => {
                let name = e.name().as_ref().to_vec();
                if name == b"metalink" {
                    is_metalink = true;
                } else if name == b"file" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"name" {
                            filename = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                } else if name == b"hash" {
                    current_hash_type.clear();
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"type" {
                            current_hash_type =
                                String::from_utf8_lossy(&attr.value).to_ascii_lowercase();
                        }
                    }
                } else if name == b"url" {
                    current_url_prio = 0;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"preference"
                            && let Ok(pref) =
                                String::from_utf8_lossy(&attr.value).trim().parse::<i64>()
                        {
                            current_url_prio = pref;
                        } else if attr.key.as_ref() == b"priority"
                            && let Ok(prio) =
                                String::from_utf8_lossy(&attr.value).trim().parse::<i64>()
                        {
                            current_url_prio = -prio;
                        }
                    }
                }
                current_tag = name;
            }
            Ok(XmlEvent::Text(ref e)) => {
                let text = String::from_utf8_lossy(e.as_ref()).trim().to_string();
                if !text.is_empty() {
                    if current_tag == b"size" {
                        if let Ok(s) = text.parse::<u64>() {
                            size = Some(s);
                        }
                    } else if current_tag == b"hash" {
                        let algo = match current_hash_type.as_str() {
                            "sha256" | "sha-256" => Some(ChecksumAlgorithm::Sha256),
                            "sha1" | "sha-1" => Some(ChecksumAlgorithm::Sha1),
                            "md5" => Some(ChecksumAlgorithm::Md5),
                            _ => None,
                        };
                        if let Some(algorithm) = algo {
                            checksum = Some(Checksum {
                                algorithm,
                                expected: text,
                            });
                        }
                    } else if current_tag == b"url" {
                        let order = urls_with_prio.len();
                        urls_with_prio.push((text, current_url_prio, order));
                    }
                }
            }
            Ok(XmlEvent::End(_)) => {
                current_tag.clear();
            }
            Ok(XmlEvent::Eof) => break,
            Err(err) => return Err(MetalinkError::Xml(err)),
            _ => {}
        }
        buf.clear();
    }

    if !is_metalink {
        return Err(MetalinkError::Invalid(
            "root element is not <metalink>".to_string(),
        ));
    }
    if filename.is_empty() {
        return Err(MetalinkError::Invalid(
            "missing file name in metalink".to_string(),
        ));
    }

    urls_with_prio.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
    let urls: Vec<String> = urls_with_prio.into_iter().map(|(u, _, _)| u).collect();

    Ok(MetalinkSpec {
        filename,
        size,
        urls,
        checksum,
    })
}
