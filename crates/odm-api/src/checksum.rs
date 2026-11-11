//! Checksum models and parsing.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Supported cryptographic checksum algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChecksumAlgorithm {
    Sha256,
    Sha1,
    Md5,
}

impl fmt::Display for ChecksumAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sha256 => write!(f, "sha256"),
            Self::Sha1 => write!(f, "sha1"),
            Self::Md5 => write!(f, "md5"),
        }
    }
}

/// A checksum algorithm combined with an expected hexadecimal digest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChecksumDto {
    pub algorithm: ChecksumAlgorithm,
    pub digest: String,
}

impl fmt::Display for ChecksumDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseChecksumError(pub String);

impl fmt::Display for ParseChecksumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid checksum format: {}", self.0)
    }
}

impl std::error::Error for ParseChecksumError {}

impl FromStr for ChecksumDto {
    type Err = ParseChecksumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if let Some((algo_str, digest_str)) = trimmed.split_once(':') {
            let algorithm = match algo_str.to_ascii_lowercase().as_str() {
                "sha256" | "sha-256" => ChecksumAlgorithm::Sha256,
                "sha1" | "sha-1" => ChecksumAlgorithm::Sha1,
                "md5" => ChecksumAlgorithm::Md5,
                other => return Err(ParseChecksumError(format!("unknown algorithm: {other}"))),
            };
            let digest = digest_str.trim().to_ascii_lowercase();
            if digest.is_empty() {
                return Err(ParseChecksumError("digest cannot be empty".to_string()));
            }
            Ok(Self { algorithm, digest })
        } else {
            // Auto-detect based on hex string length if no prefix is given
            let digest = trimmed.to_ascii_lowercase();
            let algorithm = match digest.len() {
                64 => ChecksumAlgorithm::Sha256,
                40 => ChecksumAlgorithm::Sha1,
                32 => ChecksumAlgorithm::Md5,
                len => {
                    return Err(ParseChecksumError(format!(
                        "unrecognized digest length ({len}) without algorithm prefix"
                    )));
                }
            };
            Ok(Self { algorithm, digest })
        }
    }
}
