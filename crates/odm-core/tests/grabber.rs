//! Integration tests for the site grabber core engine.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;

use axum::Router;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use odm_core::grabber::{GrabberConfig, SiteGrabber};
use tokio::net::TcpListener;

struct TestServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/robots.txt", get(robots_txt))
            .route("/index.html", get(index_html))
            .route("/page2.html", get(page2_html))
            .route("/deep/page3.html", get(page3_html))
            .route("/image1.png", get(dummy_asset))
            .route("/docs/report.pdf", get(dummy_asset))
            .route("/secret/file.zip", get(dummy_asset))
            .route("/image2.jpg", get(dummy_asset));
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { addr, handle }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn robots_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain")],
        "User-agent: *\nDisallow: /secret/\n",
    )
        .into_response()
}

async fn index_html() -> Response {
    let body = r#"<!DOCTYPE html>
<html>
<head><title>Index</title></head>
<body>
    <a href="/page2.html">Page 2</a>
    <a href="/secret/file.zip">Secret Zip</a>
    <img src="/image1.png" alt="Image 1">
    <a href="/docs/report.pdf">Report</a>
    <a href="https://other.com/ext.zip">External Link</a>
</body>
</html>"#;
    ([(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn page2_html() -> Response {
    let body = r#"<!DOCTYPE html>
<html>
<head><title>Page 2</title></head>
<body>
    <img src="/image2.jpg" alt="Image 2">
    <a href="/deep/page3.html">Page 3</a>
</body>
</html>"#;
    ([(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn page3_html() -> Response {
    let body = r#"<!DOCTYPE html>
<html>
<head><title>Page 3</title></head>
<body><p>Deep content</p></body>
</html>"#;
    ([(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn dummy_asset() -> Response {
    ([(header::CONTENT_TYPE, "application/octet-stream")], "data").into_response()
}

#[tokio::test]
async fn test_crawl_depth_1_respects_robots_and_extension_filter() {
    let server = TestServer::start().await;
    let start_url = url::Url::parse(&server.url("/index.html")).unwrap();
    let config = GrabberConfig {
        start_url: start_url.clone(),
        depth_limit: 1,
        extensions: vec!["png".to_string(), "pdf".to_string()],
        pattern: None,
        respect_robots_txt: true,
        max_pages: 50,
    };
    let grabber = SiteGrabber::new(config);
    let client = reqwest::Client::new();
    let result = grabber.crawl(&client).await.unwrap();

    assert_eq!(result.pages_crawled, 1);
    let filenames: Vec<&str> = result.files.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(filenames, vec!["image1.png", "report.pdf"]);

    for file in &result.files {
        assert_eq!(file.depth, 1);
        assert_eq!(file.found_on, start_url);
    }
}

#[tokio::test]
async fn test_crawl_depth_2_discovers_nested_pages() {
    let server = TestServer::start().await;
    let start_url = url::Url::parse(&server.url("/index.html")).unwrap();
    let page2_url = url::Url::parse(&server.url("/page2.html")).unwrap();
    let config = GrabberConfig {
        start_url: start_url.clone(),
        depth_limit: 2,
        extensions: vec!["png".to_string(), "pdf".to_string(), "jpg".to_string()],
        pattern: None,
        respect_robots_txt: true,
        max_pages: 50,
    };
    let grabber = SiteGrabber::new(config);
    let client = reqwest::Client::new();
    let result = grabber.crawl(&client).await.unwrap();

    assert_eq!(result.pages_crawled, 2);
    let filenames: Vec<&str> = result.files.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(filenames, vec!["image1.png", "report.pdf", "image2.jpg"]);

    assert_eq!(result.files[0].found_on, start_url);
    assert_eq!(result.files[0].depth, 1);

    assert_eq!(result.files[1].found_on, start_url);
    assert_eq!(result.files[1].depth, 1);

    assert_eq!(result.files[2].found_on, page2_url);
    assert_eq!(result.files[2].depth, 2);
}

#[tokio::test]
async fn test_crawl_disabling_robots_txt_collects_disallowed_files() {
    let server = TestServer::start().await;
    let start_url = url::Url::parse(&server.url("/index.html")).unwrap();
    let config = GrabberConfig {
        start_url: start_url.clone(),
        depth_limit: 1,
        extensions: vec!["zip".to_string()],
        pattern: None,
        respect_robots_txt: false,
        max_pages: 50,
    };
    let grabber = SiteGrabber::new(config);
    let client = reqwest::Client::new();
    let result = grabber.crawl(&client).await.unwrap();

    let filenames: Vec<&str> = result.files.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(filenames, vec!["file.zip"]);
    assert_eq!(result.files[0].url.as_str(), server.url("/secret/file.zip"));
}

#[tokio::test]
async fn test_crawl_wildcard_pattern_filter() {
    let server = TestServer::start().await;
    let start_url = url::Url::parse(&server.url("/index.html")).unwrap();
    let config = GrabberConfig {
        start_url: start_url.clone(),
        depth_limit: 1,
        extensions: Vec::new(),
        pattern: Some("*report*".to_string()),
        respect_robots_txt: true,
        max_pages: 50,
    };
    let grabber = SiteGrabber::new(config);
    let client = reqwest::Client::new();
    let result = grabber.crawl(&client).await.unwrap();

    let filenames: Vec<&str> = result.files.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(filenames, vec!["report.pdf"]);
}

async fn start_limit_server() -> (SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use axum::extract::{Path, State};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let hits = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route(
            "/p/{n}",
            get(
                |State(hits): State<Arc<AtomicUsize>>, Path(n): Path<u64>| async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let body = format!(
                        r#"<html><body><a href="/p/{}">a</a><a href="/p/{}">b</a></body></html>"#,
                        n + 1,
                        n + 2
                    );
                    ([(header::CONTENT_TYPE, "text/html")], body).into_response()
                },
            ),
        )
        .route(
            "/big.html",
            get(|| async {
                let mut body = String::from(r#"<html><body><a href="/found.zip">z</a>"#);
                body.push_str(&"x".repeat(11 * 1024 * 1024));
                body.push_str("</body></html>");
                ([(header::CONTENT_TYPE, "text/html")], body).into_response()
            }),
        )
        .with_state(hits.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, hits)
}

#[tokio::test]
async fn crawl_never_visits_more_than_500_pages() {
    let (addr, hits) = start_limit_server().await;
    let mut config = GrabberConfig::new(format!("http://{addr}/p/0").parse().unwrap());
    config.respect_robots_txt = false;
    config.depth_limit = u32::MAX;
    config.max_pages = 1_000_000;

    let result = SiteGrabber::new(config)
        .crawl(&reqwest::Client::new())
        .await
        .unwrap();

    assert_eq!(result.pages_crawled, 500);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 500);
}

#[tokio::test]
async fn page_over_10_mib_is_skipped() {
    let (addr, _hits) = start_limit_server().await;
    let mut config = GrabberConfig::new(format!("http://{addr}/big.html").parse().unwrap());
    config.respect_robots_txt = false;

    let result = SiteGrabber::new(config)
        .crawl(&reqwest::Client::new())
        .await
        .unwrap();

    assert!(result.files.is_empty(), "files: {:?}", result.files);
}
