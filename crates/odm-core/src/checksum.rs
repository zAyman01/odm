//! Cryptographic checksum computation and validation.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use tokio::io::AsyncReadExt;

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

/// A checksum algorithm and expected hexadecimal digest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Checksum {
    pub algorithm: ChecksumAlgorithm,
    pub expected: String,
}

impl Checksum {
    pub fn new(algorithm: ChecksumAlgorithm, expected: impl Into<String>) -> Self {
        Self {
            algorithm,
            expected: expected.into().trim().to_ascii_lowercase(),
        }
    }
}

/// Errors occurring during checksum verification.
#[derive(Debug, thiserror::Error)]
pub enum ChecksumError {
    #[error("i/o error during checksum verification: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch: expected {expected} ({algorithm}), got {actual}")]
    Mismatch {
        algorithm: ChecksumAlgorithm,
        expected: String,
        actual: String,
    },
}

/// Streamingly computes the cryptographic hash of `path` and verifies it against `checksum`.
pub async fn verify_file_checksum(path: &Path, checksum: &Checksum) -> Result<(), ChecksumError> {
    let mut file = tokio::fs::File::open(path).await?;
    // Large reads keep the disk busy; the buffer lives on the heap, not in the future.
    let mut buffer = vec![0u8; 1024 * 1024];

    let actual_hex = match checksum.algorithm {
        ChecksumAlgorithm::Sha256 => {
            let mut hasher = sha2::Sha256::new();
            loop {
                let n = file.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            format!("{:x}", hasher.finalize())
        }
        ChecksumAlgorithm::Sha1 => {
            let mut hasher = sha1::Sha1::new();
            loop {
                let n = file.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            format!("{:x}", hasher.finalize())
        }
        ChecksumAlgorithm::Md5 => {
            let mut hasher = md5::Md5::new();
            loop {
                let n = file.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            format!("{:x}", hasher.finalize())
        }
    };

    let expected_lower = checksum.expected.trim().to_ascii_lowercase();
    if actual_hex != expected_lower {
        return Err(ChecksumError::Mismatch {
            algorithm: checksum.algorithm,
            expected: expected_lower,
            actual: actual_hex,
        });
    }

    Ok(())
}
