use crate::shared::*;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(Serialize, Deserialize)]
struct PanicJob {}

struct PanicWorker;

impl oxanus::Job for PanicJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<PanicWorker>()
    }
}

#[async_trait::async_trait]
impl oxanus::Worker<PanicJob> for PanicWorker {
    type Error = std::io::Error;

    async fn process(&self, _: &PanicJob, _: &oxanus::JobContext) -> Result<(), std::io::Error> {
        panic!("test panic");
    }

    fn max_retries(&self) -> u32 {
        0
    }
}

impl oxanus::FromContext<()> for PanicWorker {
    fn from_context(_: &()) -> Self {
        Self
    }
}

#[tokio::test]
pub async fn test_panic() -> TestResult {
    let redis_pool = setup();
    let ctx = oxanus::ContextValue::new(());
    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;
    let config = oxanus::Config::new(&storage)
        .register_queue::<QueueOne>()
        .register_worker::<PanicWorker, PanicJob>()
        .exit_when_processed(1);

    storage.enqueue(QueueOne, PanicJob {}).await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 1);

    let stats = oxanus::run(config, ctx).await?;

    assert_eq!(stats.panicked, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.processed, 1);
    assert_eq!(stats.succeeded, 0);
    assert_eq!(storage.dead_count().await?, 1);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    Ok(())
}
