use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct Worker1Sec {
    id: usize,
    payload: String,
}

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

impl Worker1Sec {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct Worker2Sec {
    id: usize,
    foo: i32,
}

impl Worker2Sec {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct WorkerInstant {}

impl WorkerInstant {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct WorkerInstant2 {}

impl WorkerInstant2 {
    async fn process(&self, _: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
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

    let ctx = oxanus::Context::value(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = oxanus::Config::new(&storage.clone())
        .register_queue::<QueueOne>()
        .register_queue::<QueueTwo>()
        .register_queue::<QueueThrottled>()
        .register_worker::<Worker1Sec>()
        .register_worker::<Worker2Sec>()
        .register_worker::<WorkerInstant>()
        .register_worker::<WorkerInstant2>()
        .exit_when_processed(12);

    storage
        .enqueue(
            QueueOne,
            Worker1Sec {
                id: 1,
                payload: "test".to_string(),
            },
        )
        .await?;
    storage
        .enqueue(QueueTwo(Animal::Dog, 1), Worker2Sec { id: 2, foo: 42 })
        .await?;
    storage
        .enqueue(
            QueueOne,
            Worker1Sec {
                id: 3,
                payload: "test".to_string(),
            },
        )
        .await?;
    storage
        .enqueue(QueueTwo(Animal::Cat, 2), Worker2Sec { id: 4, foo: 44 })
        .await?;
    storage
        .enqueue_in(
            QueueOne,
            Worker1Sec {
                id: 4,
                payload: "test".to_string(),
            },
            3,
        )
        .await?;
    storage
        .enqueue_in(QueueTwo(Animal::Bird, 7), Worker2Sec { id: 5, foo: 44 }, 6)
        .await?;
    storage
        .enqueue_in(QueueTwo(Animal::Bird, 7), Worker2Sec { id: 5, foo: 44 }, 15)
        .await?;
    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant2 {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant {}).await?;
    storage.enqueue(QueueThrottled, WorkerInstant2 {}).await?;

    oxanus::run(config, ctx).await?;

    Ok(())
}
