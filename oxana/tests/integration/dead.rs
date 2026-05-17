use serde::{Deserialize, Serialize};
use testresult::TestResult;

use crate::shared::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerFailJob {}

pub struct WorkerFail;

impl oxana::Job for WorkerFailJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<WorkerFail>()
    }
}

impl oxana::FromContext<()> for WorkerFail {
    fn from_context(_ctx: &()) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl oxana::Worker<WorkerFailJob> for WorkerFail {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        _jobs: Vec<oxana::BatchItem<WorkerFailJob>>,
    ) -> Result<(), WorkerError> {
        Err(WorkerError::Generic(
            "I have nothing to live for...".to_string(),
        ))
    }

    fn retry_delay(&self, _job: &WorkerFailJob, _retries: u32) -> u64 {
        0
    }
    fn max_retries(&self, _job: &WorkerFailJob) -> u32 {
        0
    }
}

#[tokio::test]
pub async fn test_dead() -> TestResult {
    let redis_pool = setup();
    let ctx = oxana::ContextValue::new(());
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?
        .register_queue::<QueueOne>()
        .register_worker::<WorkerFail, WorkerFailJob, ()>()
        .exit_when_processed(1);

    storage.enqueue(QueueOne, WorkerFailJob {}).await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 1);

    storage.clone().run(ctx).await?;

    assert_eq!(storage.dead_count().await?, 1);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    Ok(())
}
