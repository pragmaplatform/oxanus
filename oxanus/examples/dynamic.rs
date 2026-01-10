use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerState {}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(context = WorkerState)]
struct Worker2Sec {}

impl Worker2Sec {
    async fn process(&self, _: &oxanus::Context<WorkerState>) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(prefix = "two")]
struct QueueDynamic(Animal, i32);

#[derive(Debug, Serialize)]
enum Animal {
    Dog,
    Cat,
    Bird,
}

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::Context::value(WorkerState {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = oxanus::Config::new(&storage.clone())
        .register_queue::<QueueDynamic>()
        .register_worker::<Worker2Sec>()
        .exit_when_processed(5);

    storage
        .enqueue(QueueDynamic(Animal::Cat, 2), Worker2Sec {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Dog, 1), Worker2Sec {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Cat, 2), Worker2Sec {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Bird, 1), Worker2Sec {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Dog, 1), Worker2Sec {})
        .await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
