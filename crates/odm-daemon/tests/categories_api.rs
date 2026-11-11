//! Categories REST API test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use axum::http::{Request, StatusCode};
use odm_api::{CategoryDto, ListCategoriesResponse, SetCategoryRequest};
use odm_core::{Engine, Store};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router_with_state;
use odm_daemon::state::AppState;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn categories_crud_api_lifecycle() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("daemon.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let state = AppState::new(engine, config);
    let app = build_router_with_state(state);

    // 1. GET /api/v1/categories returns list with default categories
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/categories")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: ListCategoriesResponse = serde_json::from_slice(&body).unwrap();
    assert!(list.categories.iter().any(|c| c.name == "Compressed"));
    assert!(list.categories.iter().any(|c| c.name == "Other"));

    // 2. PUT /api/v1/categories/Custom adds new category
    let put_req = Request::builder()
        .method("PUT")
        .uri("/api/v1/categories/Custom")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_string(&SetCategoryRequest {
                directory: Some("/custom/dir".to_string()),
                extensions: Some(vec!["xyz".to_string(), "abc".to_string()]),
            })
            .unwrap(),
        ))
        .unwrap();

    let put_resp = app.clone().oneshot(put_req).await.unwrap();
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_body = axum::body::to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: CategoryDto = serde_json::from_slice(&put_body).unwrap();
    assert_eq!(created.name, "Custom");
    assert_eq!(created.directory, "/custom/dir");

    // 3. DELETE /api/v1/categories/Compressed (default category) is rejected
    let del_def_req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/categories/Compressed")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(Body::empty())
        .unwrap();
    let del_def_resp = app.clone().oneshot(del_def_req).await.unwrap();
    assert_eq!(del_def_resp.status(), StatusCode::BAD_REQUEST);

    // 4. DELETE /api/v1/categories/Custom succeeds
    let del_req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/categories/Custom")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(Body::empty())
        .unwrap();
    let del_resp = app.clone().oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);
}
