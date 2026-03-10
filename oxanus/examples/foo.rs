use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize)]
struct Job1Sec {
    id: usize,
    payload: String,
}

#[derive(oxanus::Worker)]
#[oxanus(args = Job1Sec)]
struct Worker1Sec;

impl Worker1Sec {
    async fn process(&self, _job: &Job1Sec, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Job2Sec {
    id: usize,
    foo: i32,
}

#[derive(oxanus::Worker)]
#[oxanus(args = Job2Sec)]
struct Worker2Sec;

impl Worker2Sec {
    async fn process(&self, _job: &Job2Sec, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct InstantJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = InstantJob)]
struct WorkerInstant;

impl WorkerInstant {
    async fn process(
        &self,
        _job: &InstantJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct InstantJob2 {}

#[derive(oxanus::Worker)]
#[oxanus(args = InstantJob2)]
struct WorkerInstant2;

impl WorkerInstant2 {
    async fn process(
        &self,
        _job: &InstantJob2,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "one", concurrency = 1)]
struct QueueOne;

#[derive(Serialize, oxanus::Queue)]
#[oxanus(prefix = "two")]
struct QueueTwo(Animal, i32);

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "throttled")]
#[oxanus(throttle(window_ms = 1500, limit = 1))]
struct QueueThrottled;

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
    let config = ComponentRegistry::build_config(&storage).exit_when_processed(12);

    storage
        .enqueue(
            QueueOne,
            Job1Sec {
                id: 1,
                payload: "test".to_string(),
            },
        )
        .await?;
    storage
        .enqueue(QueueTwo(Animal::Dog, 1), Job2Sec { id: 2, foo: 42 })
        .await?;
    storage
        .enqueue(
            QueueOne,
            Job1Sec {
                id: 3,
                payload: "test".to_string(),
            },
        )
        .await?;
    storage
        .enqueue(QueueTwo(Animal::Cat, 2), Job2Sec { id: 4, foo: 44 })
        .await?;
    storage
        .enqueue_in(
            QueueOne,
            Job1Sec {
                id: 4,
                payload: "test".to_string(),
            },
            3,
        )
        .await?;
    storage
        .enqueue_in(QueueTwo(Animal::Bird, 7), Job2Sec { id: 5, foo: 44 }, 6)
        .await?;
    storage
        .enqueue_in(QueueTwo(Animal::Bird, 7), Job2Sec { id: 5, foo: 44 }, 15)
        .await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob2 {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob {}).await?;
    storage.enqueue(QueueThrottled, InstantJob2 {}).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
