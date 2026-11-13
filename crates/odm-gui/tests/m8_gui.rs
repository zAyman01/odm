//! GUI tests for Milestone 8: Grabber dialog and Plugins screen with security notice.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{GrabbedFileDto, GrabberResponse, PluginDto, PluginListResponse};
use odm_gui::messages::GuiEvent;
use odm_gui::state::ViewState;

#[test]
fn grabber_dialog_lifecycle_and_batch_add() {
    let mut state = ViewState::default();
    assert!(!state.grabber.is_open);

    state.open_grabber_dialog();
    assert!(state.grabber.is_open);

    // Set input fields
    state.grabber.url = "http://example.com".to_string();
    state.grabber.depth = 2;
    state.grabber.extensions = "zip, png".to_string();
    state.grabber.pattern = "test*".to_string();
    state.grabber.respect_robots = true;
    state.grabber.max_pages = 25;

    let req = state.build_grab_request().unwrap();
    assert_eq!(req.url, "http://example.com");
    assert_eq!(req.depth, Some(2));
    assert_eq!(
        req.extensions,
        Some(vec!["zip".to_string(), "png".to_string()])
    );
    assert_eq!(req.pattern.as_deref(), Some("test*"));
    assert_eq!(req.respect_robots, Some(true));
    assert_eq!(req.max_pages, Some(25));

    // Simulate crawl results arriving via event
    let resp = GrabberResponse {
        files: vec![
            GrabbedFileDto {
                url: "http://example.com/file1.zip".to_string(),
                filename: "file1.zip".to_string(),
                found_on: "http://example.com".to_string(),
                depth: 1,
            },
            GrabbedFileDto {
                url: "http://example.com/image2.png".to_string(),
                filename: "image2.png".to_string(),
                found_on: "http://example.com/page2".to_string(),
                depth: 2,
            },
        ],
        pages_crawled: 5,
    };

    state.apply_event(GuiEvent::GrabberResult(Ok(resp)));
    assert_eq!(state.grabber.files.len(), 2);
    assert_eq!(state.grabber.pages_crawled, 5);
    // By default, all files selected
    assert_eq!(state.grabber.selected_indices.len(), 2);

    let batch = state.build_grabbed_add_requests();
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].url, "http://example.com/file1.zip");
    assert_eq!(batch[1].url, "http://example.com/image2.png");

    // Toggle one file off
    state.grabber.toggle_file(0);
    assert_eq!(state.grabber.selected_indices.len(), 1);
    let batch_partial = state.build_grabbed_add_requests();
    assert_eq!(batch_partial.len(), 1);
    assert_eq!(batch_partial[0].url, "http://example.com/image2.png");

    // Deselect all
    state.grabber.deselect_all();
    assert_eq!(state.grabber.selected_indices.len(), 0);
    assert!(state.build_grabbed_add_requests().is_empty());

    // Select all
    state.grabber.select_all();
    assert_eq!(state.grabber.selected_indices.len(), 2);
}

#[test]
fn plugins_dialog_lifecycle_and_security_notice() {
    let mut state = ViewState::default();
    assert!(!state.plugins.is_open);

    state.open_plugins_dialog();
    assert!(state.plugins.is_open);
    assert!(state.plugins.has_security_notice());
    assert!(
        state
            .plugins
            .security_notice_text()
            .contains("Security Notice")
    );

    let list_resp = PluginListResponse {
        plugins: vec![PluginDto {
            name: "example-resolver".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Resolves video links".to_string()),
            role: "resolver".to_string(),
            url_patterns: vec!["*://example.com/*".to_string()],
            command: vec!["python".to_string(), "res.py".to_string()],
        }],
    };

    state.apply_event(GuiEvent::PluginsList(Ok(list_resp)));
    assert_eq!(state.plugins.plugins.len(), 1);
    assert_eq!(state.plugins.plugins[0].name, "example-resolver");
    assert_eq!(state.plugins.plugins[0].role, "resolver");
}
