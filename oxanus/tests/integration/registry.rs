use crate::shared::{WorkerError, WorkerState as WorkerContext, random_string, setup};
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "two")]
struct QueueTwo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterJob {
    pub key: String,
}

#[derive(oxanus::Worker)]
#[oxanus(args = CounterJob)]
pub struct CounterWorker {
    ctx: WorkerContext,
}

impl CounterWorker {
    async fn process(
        &self,
        job: &CounterJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        let mut redis = self.ctx.redis.get().await?;
        let _: () = redis.incr(&job.key, 1).await?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CronCounterJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = CronCounterJob)]
#[oxanus(cron(schedule = "* * * * * *", queue = QueueTwo))]
pub struct CronCounterWorker {
    ctx: WorkerContext,
}

impl CronCounterWorker {
    async fn process(
        &self,
        _job: &CronCounterJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        let mut redis = self.ctx.redis.get().await?;
        let _: () = redis.incr("test_worker:counter", 1).await?;
        Ok(())
    }
}

#[tokio::test]
pub async fn test_registry() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let _: i64 = redis_conn.del("test_worker:counter").await?;

    let ctx = oxanus::ContextValue::new(WorkerContext {
        redis: redis_pool.clone(),
    });

    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;

    let config = ComponentRegistry::build_config(&storage).exit_when_processed(2);

    assert!(config.has_registered_queue::<QueueTwo>());
    assert!(config.has_registered_worker_type::<CounterWorker>());
    assert!(config.has_registered_cron_worker_type::<CronCounterWorker>());

    storage
        .enqueue(
            QueueTwo,
            CounterJob {
                key: "test_worker:counter".to_owned(),
            },
        )
        .await?;

    oxanus::run(config, ctx).await?;

    let mut redis_conn = redis_pool.get().await?;
    let value: Option<i64> = redis_conn.get("test_worker:counter").await?;

    assert_eq!(value, Some(2));

    Ok(())
}
