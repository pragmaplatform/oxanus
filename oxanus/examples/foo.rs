use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize)]
struct Worker1SecJob {
    id: usize,
    payload: String,
}

#[derive(oxanus::Worker)]
#[oxanus(args = Worker1SecJob)]
struct Worker1Sec;

impl Worker1Sec {
    async fn process(
        &self,
        _job: &Worker1SecJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Worker2SecJob {
    id: usize,
    foo: i32,
}

#[derive(oxanus::Worker)]
#[oxanus(args = Worker2SecJob)]
struct Worker2Sec;

impl Worker2Sec {
    async fn process(
        &self,
        _job: &Worker2SecJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkerInstantJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = WorkerInstantJob)]
struct WorkerInstant;

impl WorkerInstant {
    async fn process(
        &self,
        _job: &WorkerInstantJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkerInstant2Job {}

#[derive(oxanus::Worker)]
#[oxanus(args = WorkerInstant2Job)]
struct WorkerInstant2;

impl WorkerInstant2 {
    async fn process(
        &self,
        _job: &WorkerInstant2Job,
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
            Worker1SecJob {
                id: 1,
                payload: "test".to_string(),
            },
        )
        .await?;
    storage
        .enqueue(QueueTwo(Animal::Dog, 1), Worker2SecJob { id: 2, foo: 42 })
        .await?;
    storage
        .enqueue(
            QueueOne,
            Worker1SecJob {
                id: 3,
                payload: "test".to_string(),
            },
        )
        .await?;
    storage
        .enqueue(QueueTwo(Animal::Cat, 2), Worker2SecJob { id: 4, foo: 44 })
        .await?;
    storage
        .enqueue_in(
            QueueOne,
            Worker1SecJob {
                id: 4,
                payload: "test".to_string(),
            },
            3,
        )
        .await?;
    storage
        .enqueue_in(
            QueueTwo(Animal::Bird, 7),
            Worker2SecJob { id: 5, foo: 44 },
            6,
        )
        .await?;
    storage
        .enqueue_in(
            QueueTwo(Animal::Bird, 7),
            Worker2SecJob { id: 5, foo: 44 },
            15,
        )
        .await?;
    storage.enqueue(QueueThrottled, WorkerInstantJob {}).await?;
    storage
        .enqueue(QueueThrottled, WorkerInstant2Job {})
        .await?;
    storage.enqueue(QueueThrottled, WorkerInstantJob {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstantJob {}).await?;
    storage
        .enqueue(QueueThrottled, WorkerInstant2Job {})
        .await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
