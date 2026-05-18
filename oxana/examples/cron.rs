use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
#[oxana(resurrect = false)]
struct TestJob {}

#[derive(oxana::Worker)]
#[oxana(context = WorkerContext)]
#[oxana(cron(schedule = "*/5 * * * * *", queue = QueueOne))]
struct TestWorker;

impl TestWorker {
    async fn process(&self, _job: TestJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
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

    let ctx = WorkerContext {};
    let storage = oxana::Storage::builder().build_from_env()?;
    let runtime = storage
        .runtime(ctx)
        .register::<ComponentRegistry>()
        .shutdown_on_ctrl_c();

    runtime.run().await?;

    Ok(())
}
