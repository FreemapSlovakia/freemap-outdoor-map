use crate::app::server::app_state::AppState;
use axum::{
    Json,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

const DIAGNOSTICS_PATH: &str = "/diagnostics";

pub(crate) struct InFlight {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) started_at: Instant,
    pub(crate) waiting: AtomicBool,
}

pub(crate) struct DiagnosticsState {
    inflight: Mutex<HashMap<u64, Arc<InFlight>>>,
    next_id: AtomicU64,
}

impl DiagnosticsState {
    pub(crate) fn new() -> Self {
        Self {
            inflight: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn insert(&self, method: String, url: String) -> (u64, Arc<InFlight>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = Arc::new(InFlight {
            method,
            url,
            started_at: Instant::now(),
            waiting: AtomicBool::new(false),
        });
        self.inflight.lock().unwrap().insert(id, entry.clone());
        (id, entry)
    }

    fn remove(&self, id: u64) {
        self.inflight.lock().unwrap().remove(&id);
    }

    fn snapshot(&self) -> Vec<Arc<InFlight>> {
        self.inflight.lock().unwrap().values().cloned().collect()
    }
}

tokio::task_local! {
    static CURRENT_ENTRY: Arc<InFlight>;
}

pub(crate) struct WaitingGuard {
    entry: Option<Arc<InFlight>>,
}

impl WaitingGuard {
    pub(crate) fn new() -> Self {
        let entry = CURRENT_ENTRY.try_with(|e| e.clone()).ok();
        if let Some(e) = &entry {
            e.waiting.store(true, Ordering::Relaxed);
        }
        Self { entry }
    }
}

impl Drop for WaitingGuard {
    fn drop(&mut self) {
        if let Some(e) = &self.entry {
            e.waiting.store(false, Ordering::Relaxed);
        }
    }
}

struct RemoveOnDrop {
    id: u64,
    diagnostics: Arc<DiagnosticsState>,
}

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        self.diagnostics.remove(self.id);
    }
}

pub(crate) async fn track_request(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.uri().path() == DIAGNOSTICS_PATH {
        return next.run(request).await;
    }

    let method = request.method().as_str().to_string();
    let url = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| request.uri().path().to_string());

    let diagnostics = state.diagnostics.clone();
    let (id, entry) = diagnostics.insert(method, url);
    let _guard = RemoveOnDrop {
        id,
        diagnostics: diagnostics.clone(),
    };

    CURRENT_ENTRY.scope(entry, next.run(request)).await
}

#[derive(Serialize)]
struct ActiveEntry {
    method: String,
    url: String,
    age_ms: u64,
}

#[derive(Serialize)]
struct DbPoolStatus {
    max_size: usize,
    borrowed: usize,
    available: usize,
    waiting: usize,
}

#[derive(Serialize)]
pub(crate) struct DiagnosticsResponse {
    active: Vec<ActiveEntry>,
    waiting: usize,
    db_pool: DbPoolStatus,
}

pub(crate) async fn get(State(state): State<AppState>) -> Json<DiagnosticsResponse> {
    let now = Instant::now();
    let entries = state.diagnostics.snapshot();

    let mut active: Vec<ActiveEntry> = Vec::with_capacity(entries.len());
    let mut waiting: usize = 0;

    for entry in entries {
        if entry.waiting.load(Ordering::Relaxed) {
            waiting += 1;
        } else {
            let age_ms = now.saturating_duration_since(entry.started_at).as_millis() as u64;
            active.push(ActiveEntry {
                method: entry.method.clone(),
                url: entry.url.clone(),
                age_ms,
            });
        }
    }

    active.sort_by(|a, b| b.age_ms.cmp(&a.age_ms));

    let status = state.db_pool.status();
    let db_pool = DbPoolStatus {
        max_size: status.max_size,
        borrowed: status.size.saturating_sub(status.available),
        available: status.available,
        waiting: status.waiting,
    };

    Json(DiagnosticsResponse {
        active,
        waiting,
        db_pool,
    })
}
