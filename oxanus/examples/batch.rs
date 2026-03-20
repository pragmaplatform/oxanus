use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Clone, Serialize, Deserialize, oxanus::Worker)]
struct TestWorker {
    id: u64,
}

#[derive(Debug, Serialize, Deserialize, oxanus::BatchProcessor)]
#[oxanus(batch_size = 3, batch_linger_ms = 1000)]
struct TestBatchProcessor {
    workers: Vec<TestWorker>,
}

impl TestBatchProcessor {
    async fn process_batch(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        let ids = self
            .workers
            .iter()
            .map(|worker| worker.id)
            .collect::<Vec<_>>();
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

    let ctx = oxanus::Context::value(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage)
        .with_graceful_shutdown(tokio::signal::ctrl_c())
        .exit_when_processed(10);

    storage.enqueue(QueueOne, TestWorker { id: 1 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 2 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 3 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 4 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 5 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 6 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 7 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 8 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 9 }).await?;
    storage.enqueue(QueueOne, TestWorker { id: 10 }).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
