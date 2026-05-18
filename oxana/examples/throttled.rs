use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct InstantJob {}

#[derive(oxana::Worker)]
struct InstantWorker;

impl InstantWorker {
    async fn process(&self, _job: InstantJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
#[oxana(throttle_cost = 2)]
struct Instant2Job {}

#[derive(oxana::Worker)]
struct Instant2Worker;

impl Instant2Worker {
    async fn process(
        &self,
        _job: Instant2Job,
        _ctx: &oxana::JobContext,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "throttled")]
#[oxana(throttle(window_ms = 2000, limit = 2))]
struct QueueThrottled;

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = WorkerContext {};
    let storage = oxana::Storage::builder().build_from_env()?;
    let runtime = storage
        .runtime(ctx)
        .register::<ComponentRegistry>()
        .exit_when_processed(8);

    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, Instant2Job {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, Instant2Job {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;

    runtime.run().await?;

    Ok(())
}
