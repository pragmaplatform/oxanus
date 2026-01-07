use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(unique_id = "worker2sec:{id}", on_conflict = Skip)]
struct Worker2Sec {
    id: usize,
}

impl Worker2Sec {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
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

    let ctx = oxanus::Context::value(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = oxanus::Config::new(&storage)
        .register_queue::<QueueOne>()
        .register_worker::<Worker2Sec>();

    storage.enqueue(QueueOne, Worker2Sec { id: 1 }).await?;
    storage.enqueue(QueueOne, Worker2Sec { id: 1 }).await?;
    storage.enqueue(QueueOne, Worker2Sec { id: 2 }).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
