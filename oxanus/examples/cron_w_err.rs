use rand::RngExt;
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error("Generic error: {0}")]
    GenericError(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize)]
struct CronJobArgs {}

#[derive(oxanus::Worker)]
#[oxanus(args = CronJobArgs)]
#[oxanus(max_retries = 3, retry_delay = 0)]
#[oxanus(cron(schedule = "*/10 * * * * *", queue = QueueOne))]
struct CronWorker;

impl CronWorker {
    async fn process(
        &self,
        _job: &CronJobArgs,
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
#[oxanus(key = "one")]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config =
        ComponentRegistry::build_config(&storage).with_graceful_shutdown(tokio::signal::ctrl_c());

    oxanus::run(config, ctx).await?;

    Ok(())
}
