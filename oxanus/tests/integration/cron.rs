use crate::shared::*;
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(Debug, Serialize, Deserialize)]
pub struct CronWorkerRedisCounter {}

#[async_trait::async_trait]
impl oxanus::Worker for CronWorkerRedisCounter {
    type Context = WorkerState;
    type Error = WorkerError;

    async fn process(
        &self,
        oxanus::Context { ctx, .. }: &oxanus::Context<WorkerState>,
    ) -> Result<(), WorkerError> {
        let mut redis = ctx.redis.get().await?;
        let _: () = redis.incr("cron:counter", 1).await?;
        Ok(())
    }

    fn cron_schedule() -> Option<String> {
        Some("* * * * * *".to_string())
    }
}

#[tokio::test]
pub async fn test_cron() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let _: i64 = redis_conn.del("cron:counter").await?;

    let ctx = oxanus::Context::value(WorkerState {
        redis: redis_pool.clone(),
    });

    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;
    let config = oxanus::Config::new(&storage)
        .register_cron_worker::<CronWorkerRedisCounter>(QueueOne)
        .exit_when_processed(2);

    oxanus::run(config, ctx).await?;

    let value: Option<i64> = redis_conn.get("cron:counter").await?;

    assert_eq!(value, Some(2));

    Ok(())
}
