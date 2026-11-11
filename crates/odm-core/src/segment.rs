//! Byte ranges of a segmented download.

/// One byte range of a job, with the bytes already written inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub index: u32,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

impl Segment {
    /// Inclusive length of the range.
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn is_done(&self) -> bool {
        self.downloaded >= self.len()
    }
}
