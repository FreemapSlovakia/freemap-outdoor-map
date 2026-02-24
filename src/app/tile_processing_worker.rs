use crate::app::{
    tile_coord::TileCoord,
    tile_processor::{TileProcessingConfig, TileProcessor},
};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime},
};
use tokio::sync::mpsc;

const TILE_PROCESSING_QUEUE: usize = 4096;
const INVALIDATION_REGISTER_TTL: Duration = Duration::from_secs(60);
const INVALIDATION_REGISTER_PRUNE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub(crate) enum TileProcessingSendError {
    #[error("tile processing queue closed")]
    QueueClosed,
}

#[derive(Clone)]
pub(crate) struct TileProcessingWorker {
    inner: Arc<TileProcessingInner>,
}

struct TileProcessingInner {
    tx: Mutex<Option<mpsc::Sender<TileProcessingMessage>>>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

enum TileProcessingMessage {
    SaveTile {
        data: Vec<u8>,
        coord: TileCoord,
        scale: f64,
        render_started_at: SystemTime,
        variant_index: usize,
    },
    Invalidate {
        coord: TileCoord,
        invalidated_at: SystemTime,
    },
}

impl TileProcessingWorker {
    pub(crate) fn new(config: TileProcessingConfig) -> Self {
        let (tx, mut rx) = mpsc::channel(TILE_PROCESSING_QUEUE);

        // TODO propagate error
        let mut processor = TileProcessor::new(config).expect("tile processor");

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
                        TileProcessingMessage::SaveTile {
                            data,
                            coord,
                            scale,
                            render_started_at,
                            variant_index,
                        } => processor.handle_save_tile(
                            data,
                            coord,
                            scale,
                            render_started_at,
                            variant_index,
                        ),
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
        data: Vec<u8>,
        coord: TileCoord,
        scale: f64,
        render_started_at: SystemTime,
        variant_index: usize,
    ) -> Result<(), TileProcessingSendError> {
        let tx = {
            let guard = self.inner.tx.lock().unwrap();
            guard.clone().ok_or(TileProcessingSendError::QueueClosed)?
        };

        tx.send(TileProcessingMessage::SaveTile {
            data,
            coord,
            scale,
            render_started_at,
            variant_index,
        })
        .await
        .map_err(|_| TileProcessingSendError::QueueClosed)
    }

    pub(crate) fn invalidate_blocking(
        &self,
        coord: TileCoord,
        invalidated_at: SystemTime,
    ) -> Result<(), TileProcessingSendError> {
        let tx = {
            let guard = self.inner.tx.lock().unwrap();
            guard.clone().ok_or(TileProcessingSendError::QueueClosed)?
        };

        tx.blocking_send(TileProcessingMessage::Invalidate {
            coord,
            invalidated_at,
        })
        .map_err(|_| TileProcessingSendError::QueueClosed)
    }

    pub(crate) fn shutdown(&self) {
        let tx = self.inner.tx.lock().unwrap().take();
        drop(tx);

        if let Some(handle) = self.inner.handle.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}
