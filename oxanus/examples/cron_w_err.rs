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

#[derive(Debug, Serialize, Deserialize)]
struct TestWorkerJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = TestWorkerJob)]
#[oxanus(registry = None)]
#[oxanus(max_retries = 3, retry_delay = 0)]
#[oxanus(cron(schedule = "*/10 * * * * *", queue = QueueOne))]
struct TestWorker;

impl TestWorker {
    async fn process(
        &self,
        _job: &TestWorkerJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        if rand::rng().random_bool(0.5) {
            Err(WorkerError::GenericError("foo".to_string()))
        } else {
            Ok(())
        }
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(registry = None)]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = oxanus::Config::new(&storage)
        .register_worker::<TestWorker, TestWorkerJob>()
        .with_graceful_shutdown(tokio::signal::ctrl_c());

    oxanus::run(config, ctx).await?;

    Ok(())
}
