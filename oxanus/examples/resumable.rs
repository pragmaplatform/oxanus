use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error("Generic error: {0}")]
    GenericError(String),
    #[error("Job state json error: {0}")]
    JobError(#[from] oxanus::OxanusError),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerState {}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(context = WorkerState, max_retries = 10, retry_delay = 3)]
struct ResumableTestWorker {}

impl ResumableTestWorker {
    async fn process(&self, ctx: &oxanus::Context<WorkerState>) -> Result<(), WorkerError> {
        let progress = ctx.state.get::<i32>().await?;

        dbg!(&progress);

        ctx.state.update(progress.unwrap_or(0) + 1).await?;

        if progress.unwrap_or(0) == 10 {
            Ok(())
        } else {
            Err(WorkerError::GenericError("test".to_string()))
        }
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "one", concurrency = 1)]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::Context::value(WorkerState {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = oxanus::Config::new(&storage)
        .register_queue::<QueueOne>()
        .register_worker::<ResumableTestWorker>()
        .with_graceful_shutdown(tokio::signal::ctrl_c())
        .exit_when_processed(11);

    storage.enqueue(QueueOne, ResumableTestWorker {}).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
