use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct EmailJob {
    to: String,
}

#[derive(oxana::Worker)]
#[oxana(batch_size = 4, batch_timeout_ms = 500)]
struct EmailWorker;

impl EmailWorker {
    async fn process_batch(
        &self,
        jobs: Vec<oxana::BatchItem<EmailJob>>,
    ) -> Result<(), WorkerError> {
        let recipients = jobs
            .iter()
            .map(|item| item.job.to.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!("Sending {} emails in one batch: {recipients}", jobs.len());
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "email_batches", concurrency = 4)]
struct EmailQueue;

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxana::ContextValue::new(WorkerContext {});
    let storage = oxana::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage).exit_when_processed(10);

    for i in 0..10 {
        storage
            .enqueue(
                EmailQueue,
                EmailJob {
                    to: format!("user{i}@example.com"),
                },
            )
            .await?;
    }

    oxana::run(config, ctx).await?;

    Ok(())
}
