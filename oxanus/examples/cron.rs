use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(context = WorkerContext)]
#[oxanus(cron(schedule = "*/5 * * * * *", queue = QueueOne))]
struct TestWorker {}

impl TestWorker {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        Ok(())
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

    let ctx = oxanus::Context::value(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config =
        ComponentRegistry::build_config(&storage).with_graceful_shutdown(tokio::signal::ctrl_c());

    oxanus::run(config, ctx).await?;

    Ok(())
}
