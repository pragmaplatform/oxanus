use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
#[oxana(resurrect = false)]
struct TestJob {
    label: String,
}

#[derive(oxana::Worker)]
struct TestWorker;

impl TestWorker {
    async fn process(&self, job: TestJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        tracing::info!("Processing job: {}", job.label);
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "one")]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxana::ContextValue::new(WorkerContext {});
    let storage = oxana::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage)
        .with_graceful_shutdown(tokio::signal::ctrl_c())
        .exit_when_processed(2);

    let now = Utc::now();

    storage
        .enqueue_at(
            QueueOne,
            TestJob {
                label: "30 seconds from now".into(),
            },
            now + chrono::Duration::seconds(30),
        )
        .await?;

    storage
        .enqueue_at(
            QueueOne,
            TestJob {
                label: "60 seconds from now".into(),
            },
            now + chrono::Duration::seconds(60),
        )
        .await?;

    oxana::run(config, ctx).await?;

    Ok(())
}
