use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct TestWorker {
    sleep_s: u64,
}

impl TestWorker {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_secs(self.sleep_s)).await;
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "one", concurrency = 2)]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::Context::value(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage)
        .with_graceful_shutdown(tokio::signal::ctrl_c())
        .exit_when_processed(1);

    storage
        .enqueue(QueueOne, TestWorker { sleep_s: 10 })
        .await?;
    storage.enqueue(QueueOne, TestWorker { sleep_s: 5 }).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
