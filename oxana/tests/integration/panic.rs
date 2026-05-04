use crate::shared::*;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(Debug, Serialize, Deserialize)]
struct WorkerPanicJob {}

struct WorkerPanic;

impl oxana::Job for WorkerPanicJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<WorkerPanic>()
    }
}

impl oxana::FromContext<()> for WorkerPanic {
    fn from_context(_ctx: &()) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl oxana::Worker<WorkerPanicJob> for WorkerPanic {
    type Error = std::io::Error;

    async fn run_batch(
        &self,
        _jobs: Vec<oxana::BatchItem<WorkerPanicJob>>,
    ) -> Result<(), std::io::Error> {
        panic!("test panic");
    }

    fn max_retries(&self, _job: &WorkerPanicJob) -> u32 {
        0
    }
}

#[tokio::test]
pub async fn test_panic() -> TestResult {
    let redis_pool = setup();
    let ctx = oxana::ContextValue::new(());
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;
    let config = oxana::Config::new(&storage)
        .register_queue::<QueueOne>()
        .register_worker::<WorkerPanic, WorkerPanicJob>()
        .exit_when_processed(1);

    storage.enqueue(QueueOne, WorkerPanicJob {}).await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 1);

    let stats = oxana::run(config, ctx).await?;

    assert_eq!(stats.panicked, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.processed, 1);
    assert_eq!(stats.succeeded, 0);
    assert_eq!(storage.dead_count().await?, 1);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    Ok(())
}
