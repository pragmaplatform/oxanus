use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize)]
struct Job2Sec {}

#[derive(oxanus::Worker)]
#[oxanus(args = Job2Sec)]
struct Worker2Sec;

impl Worker2Sec {
    async fn process(&self, _job: &Job2Sec, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
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

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage).exit_when_processed(5);

    storage
        .enqueue(QueueDynamic(Animal::Cat, 2), Job2Sec {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Dog, 1), Job2Sec {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Cat, 2), Job2Sec {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Bird, 1), Job2Sec {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Dog, 1), Job2Sec {})
        .await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
