use crate::shared::{WorkerError, WorkerState as WorkerContext, random_string, setup};
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "two")]
struct QueueTwo;

#[derive(Debug, Clone, Serialize, Deserialize, oxanus::Worker)]
pub struct WorkerCounter {
    pub key: String,
}

impl WorkerCounter {
    async fn process(&self, ctx: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        let mut redis = ctx.ctx.redis.get().await?;
        let _: () = redis.incr(&self.key, 1).await?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(cron(schedule = "* * * * * *", queue = QueueTwo))]
pub struct CronWorkerCounter {}

impl CronWorkerCounter {
    async fn process(&self, ctx: &oxanus::Context<WorkerContext>) -> Result<(), WorkerError> {
        let mut redis = ctx.ctx.redis.get().await?;
        let _: () = redis.incr("test_worker:counter", 1).await?;
        Ok(())
    }
}

#[tokio::test]
pub async fn test_registry() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let _: i64 = redis_conn.del("test_worker:counter").await?;

    let ctx = oxanus::Context::value(WorkerContext {
        redis: redis_pool.clone(),
    });

    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;

    let config = ComponentRegistry::build_config(&storage).exit_when_processed(2);

    // no need to manually register, here we verify they were registered
    assert!(config.has_registered_queue::<QueueTwo>());
    assert!(config.has_registered_worker::<WorkerCounter>());
    assert!(config.has_registered_cron_worker::<CronWorkerCounter>());

    storage
        .enqueue(
            QueueTwo,
            WorkerCounter {
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
