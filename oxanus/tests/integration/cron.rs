use crate::shared::*;
use deadpool_redis::redis::AsyncCommands;
use oxanus::{Queue, QueueConfig};
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(Debug, Serialize, Deserialize)]
pub struct CronCounterJob {}

pub struct CronCounterWorker {
    pub state: WorkerState,
}

impl oxanus::Job for CronCounterJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<CronCounterWorker>()
    }
}

#[async_trait::async_trait]
impl oxanus::Worker<CronCounterJob> for CronCounterWorker {
    type Error = WorkerError;

    async fn process(
        &self,
        _job: &CronCounterJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        let mut redis = self.state.redis.get().await?;
        let _: () = redis.incr("cron:counter", 1).await?;
        Ok(())
    }

    fn cron_schedule() -> Option<String> {
        Some("* * * * * *".to_string())
    }

    fn cron_queue_config() -> Option<QueueConfig> {
        Some(QueueOne::to_config())
    }
}

impl oxanus::FromContext<WorkerState> for CronCounterWorker {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

#[tokio::test]
pub async fn test_cron() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let _: i64 = redis_conn.del("cron:counter").await?;

    let ctx = oxanus::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });

    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;
    let config = oxanus::Config::new(&storage)
        .register_worker::<CronCounterWorker, CronCounterJob>()
        .exit_when_processed(2);

    oxanus::run(config, ctx).await?;

    let value: Option<i64> = redis_conn.get("cron:counter").await?;

    assert_eq!(value, Some(2));

    Ok(())
}
