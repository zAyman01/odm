//! Unit and persistence tests for download categories.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_core::Store;
use tempfile::tempdir;

#[test]
fn migration_4_seeds_default_categories() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("store.db");
    let store = Store::open(&db_path).unwrap();

    let categories = store.list_categories().unwrap();
    let names: Vec<&str> = categories.iter().map(|c| c.name.as_str()).collect();

    assert!(names.contains(&"Compressed"));
    assert!(names.contains(&"Documents"));
    assert!(names.contains(&"Music"));
    assert!(names.contains(&"Video"));
    assert!(names.contains(&"Programs"));
    assert!(names.contains(&"Other"));
}

#[test]
fn category_for_filename_matches_expected_defaults() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("store.db");
    let store = Store::open(&db_path).unwrap();

    assert_eq!(
        store.category_for_filename("archive.tar.gz").unwrap().name,
        "Compressed"
    );
    assert_eq!(
        store.category_for_filename("song.mp3").unwrap().name,
        "Music"
    );
    assert_eq!(
        store.category_for_filename("movie.mkv").unwrap().name,
        "Video"
    );
    assert_eq!(
        store.category_for_filename("installer.msi").unwrap().name,
        "Programs"
    );
    assert_eq!(
        store.category_for_filename("report.pdf").unwrap().name,
        "Documents"
    );
    assert_eq!(
        store.category_for_filename("random.unknown").unwrap().name,
        "Other"
    );
}

#[test]
fn custom_category_lifecycle() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("store.db");
    let store = Store::open(&db_path).unwrap();

    let cat = store
        .set_category(
            "Custom",
            Some("/custom/path"),
            Some(&["xyz".to_string(), "abc".to_string()]),
        )
        .unwrap();
    assert_eq!(cat.name, "Custom");
    assert_eq!(cat.directory, "/custom/path");
    assert_eq!(cat.extensions, vec!["xyz", "abc"]);

    let matched = store.category_for_filename("test.xyz").unwrap();
    assert_eq!(matched.name, "Custom");
    assert_eq!(matched.directory, "/custom/path");

    // Deleting default category is forbidden
    assert!(store.delete_category("Compressed").is_err());

    // Deleting custom category succeeds
    assert!(store.delete_category("Custom").is_ok());
    assert_eq!(
        store.category_for_filename("test.xyz").unwrap().name,
        "Other"
    );
}
