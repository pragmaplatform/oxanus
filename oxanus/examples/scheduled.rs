use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
#[oxanus(resurrect = false)]
struct TestJob {
    label: String,
}

#[derive(oxanus::Worker)]
struct TestWorker;

impl TestWorker {
    async fn process(&self, job: &TestJob, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        tracing::info!("Processing job: {}", job.label);
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "one")]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let ctx = oxanus::ContextValue::new(WorkerContext {});
    let storage = oxanus::Storage::builder().build_from_env()?;
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

    oxanus::run(config, ctx).await?;

    Ok(())
}
