use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
struct EmailJob {
    to: String,
}

#[derive(oxanus::Worker)]
#[oxanus(batch_size = 4, batch_timeout_ms = 500)]
struct EmailWorker;

impl EmailWorker {
    async fn process_batch(
        &self,
        jobs: Vec<oxanus::BatchItem<EmailJob>>,
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

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "email_batches", concurrency = 4)]
struct EmailQueue;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
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

    oxanus::run(config, ctx).await?;

    Ok(())
}
