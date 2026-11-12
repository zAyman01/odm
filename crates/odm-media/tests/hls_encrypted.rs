#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;

use aes::cipher::{BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use odm_core::backend::{
    Backend, BackendError, ControlToken, JobContext, ProgressSink, RunOutcome,
};
use odm_core::job::{Job, JobOptions, Source};
use odm_core::limiter::{Limiter, TokenBucket};
use odm_core::store::Store;
use odm_media::hls::HlsBackend;
use tempfile::TempDir;
use tokio::net::TcpListener;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

fn encrypt_segment(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    let cipher = Aes128CbcEnc::new(key.into(), iv.into());
    cipher.encrypt_padded_vec::<Pkcs7>(data)
}

fn derive_iv(media_sequence: u64, segment_index: u64) -> [u8; 16] {
    let seq = media_sequence + segment_index;
    let mut iv = [0u8; 16];
    iv[8..16].copy_from_slice(&seq.to_be_bytes());
    iv
}

const KEY_DEFAULT: [u8; 16] = [0x42; 16];
const KEY_ROTATED_B: [u8; 16] = [0x99; 16];
const IV_EXPLICIT_1: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
const IV_EXPLICIT_2: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];

struct ServerState {
    plaintext0: Vec<u8>,
    plaintext1: Vec<u8>,
    plaintext2: Vec<u8>,

    enc_seg0: Vec<u8>,
    enc_seg1: Vec<u8>,
    enc_seg2: Vec<u8>,

    rot_seg0: Vec<u8>,
    rot_seg1: Vec<u8>,
    rot_seg2: Vec<u8>,

    seq_seg0: Vec<u8>,
    seq_seg1: Vec<u8>,
    seq_seg2: Vec<u8>,
}

struct TestServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
    state: Arc<ServerState>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let plaintext0 = vec![0x11; 1024];
        let plaintext1 = vec![0x22; 2048];
        let plaintext2 = vec![0x33; 1536];

        let enc_seg0 = encrypt_segment(&plaintext0, &KEY_DEFAULT, &IV_EXPLICIT_1);
        let enc_seg1 = encrypt_segment(&plaintext1, &KEY_DEFAULT, &IV_EXPLICIT_1);
        let enc_seg2 = encrypt_segment(&plaintext2, &KEY_DEFAULT, &IV_EXPLICIT_1);

        let rot_seg0 = encrypt_segment(&plaintext0, &KEY_DEFAULT, &IV_EXPLICIT_1);
        let rot_seg1 = encrypt_segment(&plaintext1, &KEY_DEFAULT, &IV_EXPLICIT_1);
        let rot_seg2 = encrypt_segment(&plaintext2, &KEY_ROTATED_B, &IV_EXPLICIT_2);

        let seq_iv0 = derive_iv(5, 0);
        let seq_iv1 = derive_iv(5, 1);
        let seq_iv2 = derive_iv(5, 2);

        let seq_seg0 = encrypt_segment(&plaintext0, &KEY_DEFAULT, &seq_iv0);
        let seq_seg1 = encrypt_segment(&plaintext1, &KEY_DEFAULT, &seq_iv1);
        let seq_seg2 = encrypt_segment(&plaintext2, &KEY_DEFAULT, &seq_iv2);

        let state = Arc::new(ServerState {
            plaintext0,
            plaintext1,
            plaintext2,
            enc_seg0,
            enc_seg1,
            enc_seg2,
            rot_seg0,
            rot_seg1,
            rot_seg2,
            seq_seg0,
            seq_seg1,
            seq_seg2,
        });

        let app = Router::new()
            .route(
                "/key",
                get(|| async {
                    (
                        [(CONTENT_TYPE, "application/octet-stream")],
                        KEY_DEFAULT.to_vec(),
                    )
                }),
            )
            .route(
                "/keyA",
                get(|| async {
                    (
                        [(CONTENT_TYPE, "application/octet-stream")],
                        KEY_DEFAULT.to_vec(),
                    )
                }),
            )
            .route(
                "/keyB",
                get(|| async {
                    (
                        [(CONTENT_TYPE, "application/octet-stream")],
                        KEY_ROTATED_B.to_vec(),
                    )
                }),
            )
            .route("/missing-key", get(|| async { StatusCode::NOT_FOUND }))
            .route("/encrypted.m3u8", get(playlist_encrypted))
            .route("/key_rotation.m3u8", get(playlist_key_rotation))
            .route("/missing_key.m3u8", get(playlist_missing_key))
            .route("/sequence_iv.m3u8", get(playlist_sequence_iv))
            .route(
                "/enc_seg0.ts",
                get(|s: State<Arc<ServerState>>| async move { s.enc_seg0.clone() }),
            )
            .route(
                "/enc_seg1.ts",
                get(|s: State<Arc<ServerState>>| async move { s.enc_seg1.clone() }),
            )
            .route(
                "/enc_seg2.ts",
                get(|s: State<Arc<ServerState>>| async move { s.enc_seg2.clone() }),
            )
            .route(
                "/rot_seg0.ts",
                get(|s: State<Arc<ServerState>>| async move { s.rot_seg0.clone() }),
            )
            .route(
                "/rot_seg1.ts",
                get(|s: State<Arc<ServerState>>| async move { s.rot_seg1.clone() }),
            )
            .route(
                "/rot_seg2.ts",
                get(|s: State<Arc<ServerState>>| async move { s.rot_seg2.clone() }),
            )
            .route(
                "/seq_seg0.ts",
                get(|s: State<Arc<ServerState>>| async move { s.seq_seg0.clone() }),
            )
            .route(
                "/seq_seg1.ts",
                get(|s: State<Arc<ServerState>>| async move { s.seq_seg1.clone() }),
            )
            .route(
                "/seq_seg2.ts",
                get(|s: State<Arc<ServerState>>| async move { s.seq_seg2.clone() }),
            )
            .with_state(state.clone());

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            addr,
            handle,
            state,
        }
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

async fn playlist_encrypted() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-KEY:METHOD=AES-128,URI=\"/key\",IV=0x00000000000000000000000000000001\n\
#EXTINF:10.0,\n\
enc_seg0.ts\n\
#EXTINF:10.0,\n\
enc_seg1.ts\n\
#EXTINF:10.0,\n\
enc_seg2.ts\n\
#EXT-X-ENDLIST\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn playlist_key_rotation() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-KEY:METHOD=AES-128,URI=\"/keyA\",IV=0x00000000000000000000000000000001\n\
#EXTINF:10.0,\n\
rot_seg0.ts\n\
#EXTINF:10.0,\n\
rot_seg1.ts\n\
#EXT-X-KEY:METHOD=AES-128,URI=\"/keyB\",IV=0x00000000000000000000000000000002\n\
#EXTINF:10.0,\n\
rot_seg2.ts\n\
#EXT-X-ENDLIST\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn playlist_missing_key() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-KEY:METHOD=AES-128,URI=\"/missing-key\",IV=0x00000000000000000000000000000001\n\
#EXTINF:10.0,\n\
enc_seg0.ts\n\
#EXT-X-ENDLIST\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn playlist_sequence_iv() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:5\n\
#EXT-X-KEY:METHOD=AES-128,URI=\"/key\"\n\
#EXTINF:10.0,\n\
seq_seg0.ts\n\
#EXTINF:10.0,\n\
seq_seg1.ts\n\
#EXTINF:10.0,\n\
seq_seg2.ts\n\
#EXT-X-ENDLIST\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

#[tokio::test]
async fn download_encrypted_hls_decrypts_and_concatenates() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/encrypted.m3u8"));
    let temp_dir = TempDir::new().unwrap();

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (_control_handle, control) = ControlToken::new();
    let (progress, _progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);

    let expected_plaintext = [
        server.state.plaintext0.as_slice(),
        server.state.plaintext1.as_slice(),
        server.state.plaintext2.as_slice(),
    ]
    .concat();

    let target_path = job.target_path();
    assert!(target_path.exists());
    let downloaded_data = std::fs::read(&target_path).unwrap();
    assert_eq!(downloaded_data, expected_plaintext);
}

#[tokio::test]
async fn download_encrypted_hls_key_rotation() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/key_rotation.m3u8"));
    let temp_dir = TempDir::new().unwrap();

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (_control_handle, control) = ControlToken::new();
    let (progress, _progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);

    let expected_plaintext = [
        server.state.plaintext0.as_slice(),
        server.state.plaintext1.as_slice(),
        server.state.plaintext2.as_slice(),
    ]
    .concat();

    let target_path = job.target_path();
    assert!(target_path.exists());
    let downloaded_data = std::fs::read(&target_path).unwrap();
    assert_eq!(downloaded_data, expected_plaintext);
}

#[tokio::test]
async fn download_encrypted_hls_missing_key_fails() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/missing_key.m3u8"));
    let temp_dir = TempDir::new().unwrap();

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (_control_handle, control) = ControlToken::new();
    let (progress, _progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter,
    };

    let result = backend.run(ctx).await;
    match result {
        Err(BackendError::Media(msg)) => {
            assert!(
                msg.to_lowercase().contains("key"),
                "expected error message to mention 'key', got: {msg}"
            );
        }
        other => panic!("expected BackendError::Media about key, got: {other:?}"),
    }
}

#[tokio::test]
async fn download_encrypted_hls_iv_from_media_sequence() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/sequence_iv.m3u8"));
    let temp_dir = TempDir::new().unwrap();

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (_control_handle, control) = ControlToken::new();
    let (progress, _progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);

    let expected_plaintext = [
        server.state.plaintext0.as_slice(),
        server.state.plaintext1.as_slice(),
        server.state.plaintext2.as_slice(),
    ]
    .concat();

    let target_path = job.target_path();
    assert!(target_path.exists());
    let downloaded_data = std::fs::read(&target_path).unwrap();
    assert_eq!(downloaded_data, expected_plaintext);
}
