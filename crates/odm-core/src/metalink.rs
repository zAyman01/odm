//! Metalink (v3 and v4 XML) document parser.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesRef, Event as XmlEvent};
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

/// Parses a Metalink v3 (`.metalink`) or Metalink v4 (`.meta4`) XML document,
/// returning one spec for every `<file>` in it.
pub fn parse_metalink(xml: &str) -> Result<Vec<MetalinkSpec>, MetalinkError> {
    let mut reader = Reader::from_str(xml);

    let mut is_metalink = false;
    let mut files = Vec::new();
    let mut current: Option<FileBuilder> = None;

    // The text of the element being read; entity references arrive as their
    // own events, so it is collected until the element ends.
    let mut text = String::new();
    let mut current_hash_type = String::new();
    let mut current_url_prio = 0i64;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) => {
                let name = e.name().as_ref().to_vec();
                text.clear();
                if name == b"metalink" {
                    is_metalink = true;
                } else if name == b"file" {
                    let mut file = FileBuilder::default();
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"name" {
                            file.filename = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                    current = Some(file);
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
            }
            Ok(XmlEvent::Text(ref e)) => text.push_str(&String::from_utf8_lossy(e.as_ref())),
            Ok(XmlEvent::GeneralRef(ref reference)) => {
                if let Some(resolved) = resolve_reference(reference) {
                    text.push_str(&resolved);
                }
            }
            Ok(XmlEvent::End(ref e)) => {
                let name = e.name().as_ref().to_vec();
                let value = text.trim().to_string();
                text.clear();
                if name == b"file" {
                    if let Some(file) = current.take() {
                        files.push(file.finish()?);
                    }
                } else if let Some(file) = current.as_mut() {
                    if value.is_empty() {
                        // nothing to record
                    } else if name == b"size" {
                        if let Ok(size) = value.parse::<u64>() {
                            file.size = Some(size);
                        }
                    } else if name == b"hash" {
                        let algo = match current_hash_type.as_str() {
                            "sha256" | "sha-256" => Some(ChecksumAlgorithm::Sha256),
                            "sha1" | "sha-1" => Some(ChecksumAlgorithm::Sha1),
                            "md5" => Some(ChecksumAlgorithm::Md5),
                            _ => None,
                        };
                        if let Some(algorithm) = algo {
                            file.checksum = Some(Checksum {
                                algorithm,
                                expected: value,
                            });
                        }
                    } else if name == b"url" {
                        let order = file.urls.len();
                        file.urls.push((value, current_url_prio, order));
                    }
                }
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
    if files.is_empty() {
        return Err(MetalinkError::Invalid(
            "missing file name in metalink".to_string(),
        ));
    }
    Ok(files)
}

/// A `<file>` element while it is being read.
#[derive(Default)]
struct FileBuilder {
    filename: String,
    size: Option<u64>,
    checksum: Option<Checksum>,
    /// URL, priority (higher first) and document order.
    urls: Vec<(String, i64, usize)>,
}

impl FileBuilder {
    fn finish(mut self) -> Result<MetalinkSpec, MetalinkError> {
        if self.filename.is_empty() {
            return Err(MetalinkError::Invalid(
                "missing file name in metalink".to_string(),
            ));
        }
        self.urls
            .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
        Ok(MetalinkSpec {
            filename: self.filename,
            size: self.size,
            urls: self.urls.into_iter().map(|(url, _, _)| url).collect(),
            checksum: self.checksum,
        })
    }
}

/// Resolves `&amp;`-style and `&#38;`-style references to the text they stand for.
fn resolve_reference(reference: &BytesRef<'_>) -> Option<String> {
    if let Ok(Some(character)) = reference.resolve_char_ref() {
        return Some(character.to_string());
    }
    let name = reference.decode().ok()?;
    resolve_predefined_entity(&name).map(str::to_string)
}
