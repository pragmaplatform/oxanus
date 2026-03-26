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
pub struct OxanusWebState {
    pub storage: oxanus::Storage,
    pub catalog: oxanus::Catalog,
    pub base_path: String,
}

pub fn router(state: OxanusWebState) -> Router {
    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/busy", get(handlers::busy))
        .route("/queues", get(handlers::queues_list))
        .route("/cron", get(handlers::cron_jobs))
        .route("/scheduled", get(handlers::scheduled_jobs))
        .route("/dead", get(handlers::dead_jobs))
        .route("/retries", get(handlers::retry_jobs))
        .route("/queues/{queue_key}", get(handlers::queue_detail))
        .route("/queues/{queue_key}/wipe", post(handlers::wipe_queue))
        .route(
            "/queues/{queue_key}/jobs/{job_id}/delete",
            post(handlers::delete_job),
        )
        .layer(Extension(state))
}
