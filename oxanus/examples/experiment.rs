use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize)]
struct FooJob {
    id: u64,
}

#[derive(oxanus::Worker)]
#[oxanus(args = FooJob)]
struct FooWorker {
    ctx: WorkerContext,
}

impl FooWorker {
    async fn process(&self, job: &FooJob, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        dbg!(&job);
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "one")]
struct QueueOne;

#[tokio::main]
async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage)
        .with_graceful_shutdown(tokio::signal::ctrl_c())
        .exit_when_processed(1);

    storage.enqueue(QueueOne, FooJob { id: 1 }).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
