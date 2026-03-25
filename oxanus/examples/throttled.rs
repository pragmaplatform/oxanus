use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize)]
struct WorkerInstantJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = WorkerInstantJob)]
struct WorkerInstant;

impl WorkerInstant {
    async fn process(
        &self,
        _job: &WorkerInstantJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkerInstant2Job {}

#[derive(oxanus::Worker)]
#[oxanus(args = WorkerInstant2Job)]
#[oxanus(throttle_cost = 2)]
struct WorkerInstant2;

impl WorkerInstant2 {
    async fn process(
        &self,
        _job: &WorkerInstant2Job,
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

    storage
        .enqueue(QueueThrottled, WorkerInstantJob {})
        .await?;
    storage
        .enqueue(QueueThrottled, WorkerInstant2Job {})
        .await?;
    storage
        .enqueue(QueueThrottled, WorkerInstantJob {})
        .await?;
    storage
        .enqueue(QueueThrottled, WorkerInstantJob {})
        .await?;
    storage
        .enqueue(QueueThrottled, WorkerInstant2Job {})
        .await?;
    storage
        .enqueue(QueueThrottled, WorkerInstantJob {})
        .await?;
    storage
        .enqueue(QueueThrottled, WorkerInstantJob {})
        .await?;
    storage
        .enqueue(QueueThrottled, WorkerInstantJob {})
        .await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
