use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct TwoSecJob {}

#[derive(oxana::Worker)]
struct TwoSecWorker;

impl TwoSecWorker {
    async fn process(&self, _job: TwoSecJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(prefix = "two")]
struct QueueDynamic(Animal, i32);

#[derive(Debug, Serialize)]
enum Animal {
    Dog,
    Cat,
    Bird,
}

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxana::ContextValue::new(WorkerContext {});
    let storage = oxana::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage).exit_when_processed(5);

    storage
        .enqueue(QueueDynamic(Animal::Cat, 2), TwoSecJob {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Dog, 1), TwoSecJob {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Cat, 2), TwoSecJob {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Bird, 1), TwoSecJob {})
        .await?;
    storage
        .enqueue(QueueDynamic(Animal::Dog, 1), TwoSecJob {})
        .await?;

    oxana::run(config, ctx).await?;

    Ok(())
}
