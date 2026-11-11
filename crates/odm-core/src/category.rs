//! Category model and extension matching.

use serde::{Deserialize, Serialize};

/// A download category with target directory and associated file extensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub directory: String,
    pub extensions: Vec<String>,
}

impl Category {
    /// Returns true if the file extension of `filename` matches any of this category's extensions.
    pub fn matches_filename(&self, filename: &str) -> bool {
        let ext = match filename.rsplit_once('.') {
            Some((_, ext)) => ext.to_ascii_lowercase(),
            None => return false,
        };
        self.extensions
            .iter()
            .any(|e| e.to_ascii_lowercase() == ext)
    }
}
