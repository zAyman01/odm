//! Tests for batch URL bracket pattern expansion.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_core::batch::{BatchError, expand_batch_url};

#[test]
fn normal_url_without_brackets_returns_single_element() {
    let urls = expand_batch_url("https://example.com/file.zip").unwrap();
    assert_eq!(urls, vec!["https://example.com/file.zip"]);
}

#[test]
fn numerical_range_with_zero_padding() {
    let urls = expand_batch_url("https://example.com/img_[001-005].jpg").unwrap();
    assert_eq!(
        urls,
        vec![
            "https://example.com/img_001.jpg",
            "https://example.com/img_002.jpg",
            "https://example.com/img_003.jpg",
            "https://example.com/img_004.jpg",
            "https://example.com/img_005.jpg",
        ]
    );
}

#[test]
fn numerical_range_with_step() {
    let urls = expand_batch_url("https://example.com/chunk_[1-9:2].bin").unwrap();
    assert_eq!(
        urls,
        vec![
            "https://example.com/chunk_1.bin",
            "https://example.com/chunk_3.bin",
            "https://example.com/chunk_5.bin",
            "https://example.com/chunk_7.bin",
            "https://example.com/chunk_9.bin",
        ]
    );
}

#[test]
fn alphabetical_range() {
    let urls = expand_batch_url("https://example.com/part_[a-c].tar").unwrap();
    assert_eq!(
        urls,
        vec![
            "https://example.com/part_a.tar",
            "https://example.com/part_b.tar",
            "https://example.com/part_c.tar",
        ]
    );
}

#[test]
fn uppercase_alphabetical_range() {
    let urls = expand_batch_url("https://example.com/section_[X-Z].pdf").unwrap();
    assert_eq!(
        urls,
        vec![
            "https://example.com/section_X.pdf",
            "https://example.com/section_Y.pdf",
            "https://example.com/section_Z.pdf",
        ]
    );
}

#[test]
fn cartesian_product_multiple_brackets() {
    let urls = expand_batch_url("https://example.com/[01-02]_[a-b].dat").unwrap();
    assert_eq!(
        urls,
        vec![
            "https://example.com/01_a.dat",
            "https://example.com/01_b.dat",
            "https://example.com/02_a.dat",
            "https://example.com/02_b.dat",
        ]
    );
}

#[test]
fn syntax_error_unclosed_bracket() {
    let err = expand_batch_url("https://example.com/file_[01-05.zip").unwrap_err();
    assert!(matches!(err, BatchError::UnclosedBracket));
}

#[test]
fn syntax_error_invalid_range() {
    let err = expand_batch_url("https://example.com/file_[foo-bar].zip").unwrap_err();
    assert!(matches!(err, BatchError::InvalidRange(_)));
}

#[test]
fn step_zero_returns_error() {
    let err = expand_batch_url("https://example.com/file_[1-5:0].zip").unwrap_err();
    assert!(matches!(err, BatchError::InvalidStep));
}
