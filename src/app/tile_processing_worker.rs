use crate::app::{
    tile_coord::TileCoord,
    tile_processor::{TileProcessingConfig, TileProcessor},
};
use std::{
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
    tx: mpsc::Sender<TileProcessingMessage>,
}

enum TileProcessingMessage {
    SaveTile {
        data: Vec<u8>,
        coord: TileCoord,
        scale: f64,
        render_started_at: SystemTime,
    },
    Invalidate {
        coord: TileCoord,
        invalidated_at: SystemTime,
    },
}

impl TileProcessingWorker {
    pub(crate) fn new(config: TileProcessingConfig) -> Self {
        let (tx, mut rx) = mpsc::channel(TILE_PROCESSING_QUEUE);

        thread::Builder::new()
            .name("tile-processing-worker".to_string())
            .spawn(move || {
                let mut processor = TileProcessor::new(config);

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
                        } => processor.handle_save_tile(data, coord, scale, render_started_at),
                        TileProcessingMessage::Invalidate {
                            coord,
                            invalidated_at,
                        } => processor.handle_invalidation(coord, invalidated_at),
                    }
                }
            })
            .expect("spawn tile processing worker");

        Self { tx }
    }

    pub(crate) async fn save_tile(
        &self,
        data: Vec<u8>,
        coord: TileCoord,
        scale: f64,
        render_started_at: SystemTime,
    ) -> Result<(), TileProcessingSendError> {
        self.tx
            .send(TileProcessingMessage::SaveTile {
                data,
                coord,
                scale,
                render_started_at,
            })
            .await
            .map_err(|_| TileProcessingSendError::QueueClosed)
    }

    pub(crate) fn invalidate_blocking(
        &self,
        coord: TileCoord,
        invalidated_at: SystemTime,
    ) -> Result<(), TileProcessingSendError> {
        self.tx
            .blocking_send(TileProcessingMessage::Invalidate {
                coord,
                invalidated_at,
            })
            .map_err(|_| TileProcessingSendError::QueueClosed)
    }
}
