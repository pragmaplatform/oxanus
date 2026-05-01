use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
#[oxanus(unique_id = "worker2sec:{id}", on_conflict = Skip)]
struct TwoSecJob {
    id: usize,
}

#[derive(oxanus::Worker)]
struct TwoSecWorker;

impl TwoSecWorker {
    async fn process(
        &self,
        _job: &TwoSecJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
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

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage);

    storage.enqueue(QueueOne, TwoSecJob { id: 1 }).await?;
    storage.enqueue(QueueOne, TwoSecJob { id: 1 }).await?;
    storage.enqueue(QueueOne, TwoSecJob { id: 2 }).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
