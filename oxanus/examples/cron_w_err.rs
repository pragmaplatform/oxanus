use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error("Generic error: {0}")]
    GenericError(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(registry = None)]
#[oxanus(max_retries = 3, retry_delay = 0)]
#[oxanus(cron(schedule = "*/10 * * * * *"))]
struct TestWorker {}

impl TestWorker {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        if rand::rng().random_bool(0.5) {
            Err(WorkerError::GenericError("foo".to_string()))
        } else {
            Ok(())
        }
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(registry = None)]
#[oxanus(prefix = "two")]
struct QueueDynamic(i32);

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::Context::value(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = oxanus::Config::new(&storage)
        .register_cron_worker::<TestWorker>("", QueueDynamic(2))
        .with_graceful_shutdown(tokio::signal::ctrl_c());

    oxanus::run(config, ctx).await?;

    Ok(())
}
