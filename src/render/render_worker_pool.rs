use crate::render::{
    self, RenderRequest, layers::load_hillshading_datasets, render::RenderError, svg_repo::SvgRepo,
};
use geo::Geometry;
use postgres::NoTls;
use r2d2_postgres::PostgresConnectionManager;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use tokio::sync::{mpsc, oneshot};

struct RenderTask {
    request: RenderRequest,
    resp_tx: oneshot::Sender<Result<Vec<u8>, ReError>>,
}

pub(crate) struct RenderWorkerPool {
    tx: Mutex<Option<mpsc::Sender<RenderTask>>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ReError {
    #[error(transparent)]
    RenderError(#[from] RenderError),

    #[error(transparent)]
    ConnectionPoolError(#[from] r2d2::Error),

    #[error("worker response dropped: {0}")]
    RecvError(#[from] oneshot::error::RecvError),

    #[error("worker queue closed")]
    QueueClosed,
}

impl RenderWorkerPool {
    pub(crate) fn new(
        pool: r2d2::Pool<PostgresConnectionManager<NoTls>>,
        worker_count: usize,
        svg_base_path: Arc<Path>,
        hillshading_base_path: Arc<Path>,
        coverage_geometry: Option<Geometry>,
    ) -> Self {
        let queue_size = worker_count.max(1) * 2;
        let (tx, rx) = mpsc::channel(queue_size);
        let rx = Arc::new(Mutex::new(rx));
        let mut workers = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let rx = rx.clone();
            let pool = pool.clone();
            let svg_base_path = svg_base_path.clone();
            let hillshading_base_path = hillshading_base_path.clone();
            let coverage_geometry = coverage_geometry.clone();

            let handle = std::thread::Builder::new()
                .name(format!("render-worker-{worker_id}"))
                .spawn(move || {
                    let mut svg_repo = SvgRepo::new(svg_base_path.as_ref().to_path_buf());

                    let mut hillshading_datasets =
                        Some(load_hillshading_datasets(&*hillshading_base_path));

                    loop {
                        let task = {
                            let mut guard = rx.lock().unwrap();
                            guard.blocking_recv()
                        };

                        let Some(RenderTask { request, resp_tx }) = task else {
                            break;
                        };

                        let result = pool.get().map_err(ReError::from).and_then(|mut client| {
                            render::render::render(
                                &request,
                                &mut client,
                                &mut svg_repo,
                                hillshading_datasets.as_mut(),
                                coverage_geometry.as_ref(),
                            )
                            .map_err(ReError::from)
                        });

                        // Ignore send errors (client dropped).
                        let _ = resp_tx.send(result);
                    }
                });

            workers.push(handle.expect("render worker spawn"));
        }

        Self {
            tx: Mutex::new(Some(tx)),
            workers: Mutex::new(workers),
        }
    }

    pub(crate) async fn render(&self, request: RenderRequest) -> Result<Vec<u8>, ReError> {
        let (resp_tx, resp_rx) = oneshot::channel();

        let tx = {
            let guard = self.tx.lock().unwrap();
            guard.clone().ok_or(ReError::QueueClosed)?
        };

        tx.send(RenderTask { request, resp_tx })
            .await
            .map_err(|_| ReError::QueueClosed)?;

        resp_rx.await?
    }

    pub(crate) fn shutdown(&self) {
        let tx = self.tx.lock().unwrap().take();
        drop(tx);

        let mut workers = self.workers.lock().unwrap();
        for handle in workers.drain(..) {
            let _ = handle.join();
        }
    }
}
