mod error;
mod filters;
mod handlers;
mod templates;

use axum::{
    Router,
    extract::Extension,
    routing::{get, post},
};

const JOBS_PER_PAGE: usize = 50;

#[derive(Clone)]
pub struct OxanaWebState {
    pub storage: oxana::Storage,
    pub catalog: oxana::Catalog,
    pub base_path: String,
}

impl OxanaWebState {
    pub fn new(storage: oxana::Storage, catalog: oxana::Catalog, base_path: String) -> Self {
        Self {
            storage,
            catalog,
            base_path,
        }
    }
}

pub fn router(state: OxanaWebState) -> Router {
    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/busy", get(handlers::busy))
        .route("/queues", get(handlers::queues_list))
        .route("/metrics", get(handlers::metrics))
        .route("/metrics/job", get(handlers::metric_detail))
        .route("/cron", get(handlers::cron_jobs))
        .route("/cron/enqueue", post(handlers::enqueue_cron_job))
        .route("/on-demand", get(handlers::on_demand_jobs))
        .route("/on-demand/enqueue", post(handlers::enqueue_on_demand_job))
        .route("/scheduled", get(handlers::scheduled_jobs))
        .route("/dead", get(handlers::dead_jobs))
        .route("/dead/wipe", post(handlers::wipe_dead))
        .route("/retries", get(handlers::retry_jobs))
        .route("/jobs/{job_id}", get(handlers::job_detail))
        .route("/queues/{queue_key}", get(handlers::queue_detail))
        .route("/enqueue", post(handlers::enqueue_job))
        .route("/queues/{queue_key}/pause", post(handlers::pause_queue))
        .route("/queues/{queue_key}/unpause", post(handlers::unpause_queue))
        .route("/queues/{queue_key}/wipe", post(handlers::wipe_queue))
        .route(
            "/queues/{queue_key}/jobs/{job_id}/delete",
            post(handlers::delete_job),
        )
        .layer(Extension(state))
}
