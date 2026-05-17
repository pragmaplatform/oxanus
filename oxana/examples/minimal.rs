use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct TestJob {
    sleep_s: u64,
}

#[derive(oxana::Worker)]
struct TestWorker;

impl TestWorker {
    async fn process(&self, job: TestJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_secs(job.sleep_s)).await;
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "one", concurrency = 2)]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxana::ContextValue::new(WorkerContext {});
    let storage = oxana::Storage::builder().build_from_env()?;
    let storage = storage
        .register::<ComponentRegistry>()
        .with_graceful_shutdown(tokio::signal::ctrl_c())
        .exit_when_processed(1);

    storage.enqueue(QueueOne, TestJob { sleep_s: 10 }).await?;
    storage.enqueue(QueueOne, TestJob { sleep_s: 5 }).await?;

    storage.clone().run(ctx).await?;

    Ok(())
}
