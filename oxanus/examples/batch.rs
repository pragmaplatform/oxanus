use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestJob {
    id: u64,
}

#[derive(oxanus::Worker)]
#[oxanus(args = TestJob)]
struct TestWorker;

impl TestWorker {
    async fn process(&self, _job: &TestJob, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(oxanus::BatchProcessor)]
#[oxanus(batch_size = 3, batch_linger_ms = 1000)]
struct TestBatchProcessor {
    jobs: Vec<TestJob>,
}

impl TestBatchProcessor {
    async fn process_batch(&self, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        let ids = self.jobs.iter().map(|job| job.id).collect::<Vec<_>>();
        dbg!(&ids);
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "one", concurrency = 1)]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage)
        .with_graceful_shutdown(tokio::signal::ctrl_c())
        .exit_when_processed(10);

    storage.enqueue(QueueOne, TestJob { id: 1 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 2 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 3 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 4 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 5 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 6 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 7 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 8 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 9 }).await?;
    storage.enqueue(QueueOne, TestJob { id: 10 }).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
