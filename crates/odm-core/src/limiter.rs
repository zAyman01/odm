//! Shared token-bucket rate limiting.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::{Instant, sleep};

struct BucketState {
    tokens: f64,
    last_refill: Option<Instant>,
}

/// A token bucket that refills at a byte rate.
pub struct TokenBucket {
    rate: AtomicU64,
    state: Mutex<BucketState>,
}

impl TokenBucket {
    /// Creates a bucket refilling at `bytes_per_second`; zero means unlimited.
    pub fn new(bytes_per_second: u64) -> Self {
        Self {
            rate: AtomicU64::new(bytes_per_second),
            state: Mutex::new(BucketState {
                tokens: 0.0,
                last_refill: None,
            }),
        }
    }

    /// Creates a bucket without a rate limit.
    pub fn unlimited() -> Self {
        Self::new(0)
    }

    /// Changes the refill rate of a bucket that is already in use; `None` or
    /// zero means unlimited. Waiters see the new rate on their next check.
    pub fn set_rate(&self, bytes_per_second: Option<u64>) {
        self.rate
            .store(bytes_per_second.unwrap_or(0), Ordering::Relaxed);
    }

    /// Takes `bytes` tokens, waiting until the bucket has refilled enough.
    pub async fn acquire(&self, bytes: u64) {
        loop {
            let rate = self.rate.load(Ordering::Relaxed);
            if rate == 0 || bytes == 0 {
                return;
            }
            let wait = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let now = Instant::now();
                let elapsed = state
                    .last_refill
                    .map(|last_refill| now.saturating_duration_since(last_refill))
                    .unwrap_or_default();
                state.last_refill = Some(now);

                let rate = rate as f64;
                let cap = rate.max(bytes as f64);
                state.tokens = (state.tokens + elapsed.as_secs_f64() * rate).min(cap);

                if state.tokens >= bytes as f64 {
                    state.tokens -= bytes as f64;
                    return;
                }

                let missing = bytes as f64 - state.tokens;
                Duration::from_secs_f64(missing / rate).max(Duration::from_nanos(1))
            };
            sleep(wait).await;
        }
    }
}

/// A global bucket plus an optional per-job bucket.
#[derive(Clone)]
pub struct Limiter {
    global: Arc<TokenBucket>,
    job: Option<Arc<TokenBucket>>,
}

impl Limiter {
    /// Combines the global bucket with an optional per-job rate.
    pub fn new(global: Arc<TokenBucket>, job_limit: Option<u64>) -> Self {
        let job = job_limit
            .filter(|rate| *rate > 0)
            .map(|rate| Arc::new(TokenBucket::new(rate)));
        Self { global, job }
    }

    /// Takes tokens from the global bucket, then from the job bucket.
    pub async fn acquire(&self, bytes: u64) {
        self.global.acquire(bytes).await;
        if let Some(job) = &self.job {
            job.acquire(bytes).await;
        }
    }
}
