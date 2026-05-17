use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "worker2sec:{id}", on_conflict = Skip)]
struct TwoSecJob {
    id: usize,
}

#[derive(oxana::Worker)]
struct TwoSecWorker;

impl TwoSecWorker {
    async fn process(&self, _job: TwoSecJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "one")]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxana::ContextValue::new(WorkerContext {});
    let storage = oxana::Storage::builder().build_from_env()?;
    let storage = storage.register::<ComponentRegistry>();

    storage.enqueue(QueueOne, TwoSecJob { id: 1 }).await?;
    storage.enqueue(QueueOne, TwoSecJob { id: 1 }).await?;
    storage.enqueue(QueueOne, TwoSecJob { id: 2 }).await?;

    storage.clone().run(ctx).await?;

    Ok(())
}
