use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize)]
struct InstantJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = InstantJob)]
struct WorkerInstant;

impl WorkerInstant {
    async fn process(
        &self,
        _job: &InstantJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct InstantJob2 {}

#[derive(oxanus::Worker)]
#[oxanus(args = InstantJob2)]
struct WorkerInstant2;

impl WorkerInstant2 {
    async fn process(
        &self,
        _job: &InstantJob2,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
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

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage).exit_when_processed(8);

    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob2 {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob2 {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
