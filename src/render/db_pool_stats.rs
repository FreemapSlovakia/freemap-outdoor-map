//! How long render queries wait for a database connection.

use deadpool_postgres::{Object, Pool, PoolError};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// A wait this long is logged as it happens: every pooled connection stayed busy for it.
const SLOW_WAIT: Duration = Duration::from_secs(1);

/// Shorter waits are just the pool handing out an idle connection.
const MEANINGFUL_WAIT: Duration = Duration::from_millis(10);

static GETS: AtomicU64 = AtomicU64::new(0);
static WAITS: AtomicU64 = AtomicU64::new(0);
static WAIT_TOTAL_MICROS: AtomicU64 = AtomicU64::new(0);
static WAIT_MAX_MICROS: AtomicU64 = AtomicU64::new(0);

/// Takes a connection for the query of `layer`, recording how long it had to wait.
pub async fn get(pool: &Pool, layer: &str) -> Result<Object, PoolError> {
    let started = Instant::now();

    let conn = pool.get().await;

    let waited = started.elapsed();

    GETS.fetch_add(1, Ordering::Relaxed);

    if waited >= MEANINGFUL_WAIT {
        let micros = u64::try_from(waited.as_micros()).unwrap_or(u64::MAX);

        WAITS.fetch_add(1, Ordering::Relaxed);
        WAIT_TOTAL_MICROS.fetch_add(micros, Ordering::Relaxed);
        WAIT_MAX_MICROS.fetch_max(micros, Ordering::Relaxed);

        if waited >= SLOW_WAIT {
            let status = pool.status();

            eprintln!(
                "db pool: {layer} waited {:.1}s for a connection (size {}/{}, {} available, {} waiting)",
                waited.as_secs_f64(),
                status.size,
                status.max_size,
                status.available,
                status.waiting
            );
        }
    }

    conn
}

/// Prints the counters since the previous call with the pool's current state, and resets them.
pub fn log_stats(pool: &Pool, interval: Duration) {
    let gets = GETS.swap(0, Ordering::Relaxed);
    let waits = WAITS.swap(0, Ordering::Relaxed);
    let wait_total = WAIT_TOTAL_MICROS.swap(0, Ordering::Relaxed);
    let wait_max = WAIT_MAX_MICROS.swap(0, Ordering::Relaxed);

    if gets == 0 {
        return;
    }

    let status = pool.status();

    println!(
        "db pool, last {}s: gets={gets} waits={waits} wait_total={:.1}s wait_max={:.2}s size={}/{} available={} waiting={}",
        interval.as_secs(),
        wait_total as f64 / 1e6,
        wait_max as f64 / 1e6,
        status.size,
        status.max_size,
        status.available,
        status.waiting
    );
}
