use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error("Generic error: {0}")]
    GenericError(String),
    #[error("Job state json error: {0}")]
    JobError(#[from] oxana::OxanaError),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct ResumableTestJob {}

#[derive(oxana::Worker)]
#[oxana(max_retries = 10, retry_delay = 3)]
struct ResumableTestWorker;

impl ResumableTestWorker {
    async fn process(
        &self,
        _job: ResumableTestJob,
        ctx: &oxana::JobContext,
    ) -> Result<(), WorkerError> {
        let progress = ctx.state.get::<i32>().await?;

        dbg!(&progress);

        ctx.state.update(progress.unwrap_or(0) + 1).await?;

        if progress.unwrap_or(0) == 10 {
            Ok(())
        } else {
            Err(WorkerError::GenericError("test".to_string()))
        }
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "one", concurrency = 1)]
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
        .shutdown_on_ctrl_c()
        .exit_when_processed(11);

    storage.enqueue(QueueOne, ResumableTestJob {}).await?;

    runtime.run().await?;

    Ok(())
}
