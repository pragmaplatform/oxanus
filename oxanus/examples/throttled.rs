use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct WorkerInstant {}

impl WorkerInstant {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct WorkerInstant2 {}

impl WorkerInstant2 {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "throttled")]
#[oxanus(throttle(window_ms = 2000, limit = 2))]
struct QueueThrottled;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::Context::value(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage).exit_when_processed(8);

    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant2 {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant2 {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
