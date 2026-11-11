//! Token-bucket limiter and job option defaults.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use odm_core::{JobOptions, Limiter, TokenBucket};
use tokio::time::Instant;

#[tokio::test(start_paused = true)]
async fn token_bucket_waits_for_the_configured_rate() {
    let bucket = TokenBucket::new(1000);
    let start = Instant::now();

    bucket.acquire(500).await;
    bucket.acquire(500).await;

    assert_eq!(start.elapsed(), Duration::from_millis(1000));
}

#[tokio::test(start_paused = true)]
async fn unlimited_bucket_does_not_wait() {
    let bucket = TokenBucket::new(0);
    let start = Instant::now();

    bucket.acquire(1_000_000).await;

    assert_eq!(start.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn limiter_applies_global_and_per_job_limits() {
    let limiter = Limiter::new(Arc::new(TokenBucket::new(1000)), Some(500));
    let start = Instant::now();

    limiter.acquire(500).await;

    assert_eq!(start.elapsed(), Duration::from_millis(1500));
}

#[test]
fn job_options_deserialize_with_new_field_defaults() {
    let options: JobOptions =
        serde_json::from_str(r#"{"headers":[],"user_agent":null,"referer":null,"cookies":null}"#)
            .unwrap();

    assert_eq!(options.segments, None);
    assert_eq!(options.speed_limit, None);
}
