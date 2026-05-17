use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct OneSecJob {
    id: usize,
    payload: String,
}

#[derive(oxana::Worker)]
struct OneSecWorker;

impl OneSecWorker {
    async fn process(&self, _job: OneSecJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct TwoSecJob {
    id: usize,
    foo: i32,
}

#[derive(oxana::Worker)]
struct TwoSecWorker;

impl TwoSecWorker {
    async fn process(&self, _job: TwoSecJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct InstantJob {}

#[derive(oxana::Worker)]
struct InstantWorker;

impl InstantWorker {
    async fn process(&self, _job: InstantJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct Instant2Job {}

#[derive(oxana::Worker)]
struct Instant2Worker;

impl Instant2Worker {
    async fn process(
        &self,
        _job: Instant2Job,
        _ctx: &oxana::JobContext,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "one", concurrency = 1)]
struct QueueOne;

#[derive(Serialize, oxana::Queue)]
#[oxana(prefix = "two")]
struct QueueTwo(Animal, i32);

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "throttled")]
#[oxana(throttle(window_ms = 1500, limit = 1))]
struct QueueThrottled;

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
    let storage = storage
        .register::<ComponentRegistry>()
        .exit_when_processed(12);

    storage
        .enqueue(
            QueueOne,
            OneSecJob {
                id: 1,
                payload: "test".to_string(),
            },
        )
        .await?;
    storage
        .enqueue(QueueTwo(Animal::Dog, 1), TwoSecJob { id: 2, foo: 42 })
        .await?;
    storage
        .enqueue(
            QueueOne,
            OneSecJob {
                id: 3,
                payload: "test".to_string(),
            },
        )
        .await?;
    storage
        .enqueue(QueueTwo(Animal::Cat, 2), TwoSecJob { id: 4, foo: 44 })
        .await?;
    storage
        .enqueue_in(
            QueueOne,
            OneSecJob {
                id: 4,
                payload: "test".to_string(),
            },
            3,
        )
        .await?;
    storage
        .enqueue_in(QueueTwo(Animal::Bird, 7), TwoSecJob { id: 5, foo: 44 }, 6)
        .await?;
    storage
        .enqueue_in(QueueTwo(Animal::Bird, 7), TwoSecJob { id: 5, foo: 44 }, 15)
        .await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, Instant2Job {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, Instant2Job {}).await?;

    storage.clone().run(ctx).await?;

    Ok(())
}
