use rand::RngExt;
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error("Generic error: {0}")]
    GenericError(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct TestJob {}

#[derive(oxana::Worker)]
#[oxana(registry = None)]
#[oxana(max_retries = 3, retry_delay = 0)]
#[oxana(cron(schedule = "*/10 * * * * *", queue = QueueOne))]
struct TestWorker;

impl TestWorker {
    async fn process(&self, _job: TestJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        if rand::rng().random_bool(0.5) {
            Err(WorkerError::GenericError("foo".to_string()))
        } else {
            Ok(())
        }
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(registry = None)]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxana::ContextValue::new(WorkerContext {});
    let storage = oxana::Storage::builder()
        .build_from_env()?
        .register_worker::<TestWorker, TestJob, WorkerContext>()
        .with_graceful_shutdown(tokio::signal::ctrl_c());

    storage.clone().run(ctx).await?;

    Ok(())
}
