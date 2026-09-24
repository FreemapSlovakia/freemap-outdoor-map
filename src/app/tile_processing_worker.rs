use crate::{
    app::{
        tile_coord::TileCoord,
        tile_processor::{TileProcessingConfig, TileProcessor},
    },
    render::Attribution,
};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime},
};
use tokio::sync::mpsc;

const TILE_PROCESSING_QUEUE: usize = 4096;
const INVALIDATION_REGISTER_TTL: Duration = Duration::from_mins(1);
const INVALIDATION_REGISTER_PRUNE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum TileProcessingSendError {
    #[error("tile processing queue closed")]
    QueueClosed,
}

#[derive(Clone)]
pub struct TileProcessingWorker {
    inner: Arc<TileProcessingInner>,
}

struct TileProcessingInner {
    tx: Mutex<Option<mpsc::Sender<TileProcessingMessage>>>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

/// One finished tile on its way to the cache.
pub(super) struct SaveTile {
    pub(super) data: Vec<u8>,
    pub(super) attribution: Attribution,
    pub(super) coord: TileCoord,
    pub(super) scale: f64,
    pub(super) render_started_at: SystemTime,
    pub(super) variant_index: usize,
    /// Nothing was painted, so the cache records a mark instead of a file.
    pub(super) blank: bool,
}

enum TileProcessingMessage {
    SaveTile(SaveTile),
    Invalidate {
        coord: TileCoord,
        invalidated_at: SystemTime,
    },
}

impl TileProcessingWorker {
    pub(crate) fn new(config: TileProcessingConfig) -> Self {
        let (tx, mut rx) = mpsc::channel(TILE_PROCESSING_QUEUE);

        let mut processor = TileProcessor::new(config);

        let handle = thread::Builder::new()
            .name("tile-processing-worker".to_string())
            .spawn(move || {
                while let Some(message) = rx.blocking_recv() {
                    let now = SystemTime::now();

                    if now
                        .duration_since(processor.last_prune())
                        .unwrap_or(Duration::ZERO)
                        >= INVALIDATION_REGISTER_PRUNE_INTERVAL
                    {
                        processor.prune_invalidation_register(now, INVALIDATION_REGISTER_TTL);
                        processor.set_last_prune(now);
                    }

                    match message {
                        TileProcessingMessage::SaveTile(tile) => {
                            processor.handle_save_tile(tile);
                        }
                        TileProcessingMessage::Invalidate {
                            coord,
                            invalidated_at,
                        } => processor.handle_invalidation(coord, invalidated_at),
                    }
                }
            })
            .expect("spawn tile processing worker");

        Self {
            inner: Arc::new(TileProcessingInner {
                tx: Mutex::new(Some(tx)),
                handle: Mutex::new(Some(handle)),
            }),
        }
    }

    pub(crate) async fn save_tile(
        &self,
        tile: SaveTile,
    ) -> Result<(), TileProcessingSendError> {
        let tx = {
            let guard = self.inner.tx.lock().expect("mutex not poisoned");
            guard.clone().ok_or(TileProcessingSendError::QueueClosed)?
        };

        tx.send(TileProcessingMessage::SaveTile(tile))
        .await
        .map_err(|_| TileProcessingSendError::QueueClosed)
    }

    pub(crate) fn invalidate_blocking(
        &self,
        coord: TileCoord,
        invalidated_at: SystemTime,
    ) -> Result<(), TileProcessingSendError> {
        let tx = {
            let guard = self.inner.tx.lock().expect("mutex not poisoned");
            guard.clone().ok_or(TileProcessingSendError::QueueClosed)?
        };

        tx.blocking_send(TileProcessingMessage::Invalidate {
            coord,
            invalidated_at,
        })
        .map_err(|_| TileProcessingSendError::QueueClosed)
    }

    pub(crate) fn shutdown(&self) {
        let tx = self.inner.tx.lock().expect("mutex not poisoned").take();
        drop(tx);

        let handle = self.inner.handle.lock().expect("mutex not poisoned").take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}
