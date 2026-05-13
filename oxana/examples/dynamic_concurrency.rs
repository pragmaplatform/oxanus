use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct OneSecJob {
    id: usize,
}

#[derive(oxana::Worker)]
struct OneSecWorker;

impl OneSecWorker {
    async fn process(&self, job: OneSecJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        println!("job {} started", job.id);
        tokio::time::sleep(Duration::from_secs(1)).await;
        println!("job {} finished", job.id);
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "dynamic_concurrency", concurrency = Dynamic(1))]
struct DynamicConcurrencyQueue;

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxana::ContextValue::new(WorkerContext {});
    let storage = oxana::Storage::builder().build_from_env()?;

    storage
        .set_queue_config(
            DynamicConcurrencyQueue,
            &oxana::QueueRuntimeConfig {
                concurrency: Some(1),
                state: oxana::QueueState::Active,
            },
        )
        .await?;

    let config = ComponentRegistry::build_config(&storage).exit_when_processed(30);

    for id in 1..=30 {
        storage
            .enqueue(DynamicConcurrencyQueue, OneSecJob { id })
            .await?;
    }

    let update_storage = storage.clone();
    let handle = tokio::runtime::Handle::current();
    let update_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));

        handle.block_on(async move {
            update_storage
                .set_queue_concurrency(DynamicConcurrencyQueue, 0)
                .await?;
            println!("processing has been paused");

            tokio::time::sleep(Duration::from_secs(10)).await;
            println!("processing is resuming");

            update_storage
                .set_queue_concurrency(DynamicConcurrencyQueue, 10)
                .await
        })
    });

    let run_result = oxana::run(config, ctx).await;
    let update_result = update_thread.join().map_err(|_| {
        oxana::OxanaError::GenericError("concurrency update thread panicked".into())
    })?;
    update_result?;
    run_result?;

    Ok(())
}
