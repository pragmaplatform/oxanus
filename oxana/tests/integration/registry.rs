use crate::shared::{WorkerError, WorkerState as WorkerContext, random_string, setup};
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "two")]
struct QueueTwo;

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
#[oxana(worker = WorkerCounter)]
#[oxana(on_demand)]
pub struct WorkerCounterJob {
    pub key: String,
}

#[derive(oxana::Worker)]
pub struct WorkerCounter {
    ctx: WorkerContext,
}

impl WorkerCounter {
    async fn process(
        &self,
        job: WorkerCounterJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), WorkerError> {
        let mut redis = self.ctx.redis.get().await?;
        let _: () = redis.incr(&job.key, 1).await?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
#[oxana(worker = CronWorkerCounter)]
pub struct CronWorkerCounterJob {}

#[derive(oxana::Worker)]
#[oxana(cron(schedule = "* * * * * *", queue = QueueTwo))]
pub struct CronWorkerCounter {
    ctx: WorkerContext,
}

impl CronWorkerCounter {
    async fn process(
        &self,
        _job: CronWorkerCounterJob,
        _ctx: &oxana::JobContext,
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

    let ctx = oxana::ContextValue::new(WorkerContext {
        redis: redis_pool.clone(),
    });

    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;

    let config = ComponentRegistry::build_config(&storage).exit_when_processed(2);

    // no need to manually register, here we verify they were registered
    assert!(config.has_registered_queue::<QueueTwo>());
    assert!(config.has_registered_worker_type::<WorkerCounter>());
    assert!(config.has_registered_cron_worker_type::<CronWorkerCounter>());
    assert!(config.catalog().on_demand_jobs.iter().any(|job| {
        job.name == std::any::type_name::<WorkerCounter>()
            && job.args_template == serde_json::json!({ "key": "" })
    }));

    storage
        .enqueue(
            QueueTwo,
            WorkerCounterJob {
                key: "test_worker:counter".to_owned(),
            },
        )
        .await?;

    oxana::run(config, ctx).await?;

    let mut redis_conn = redis_pool.get().await?;
    let value: Option<i64> = redis_conn.get("test_worker:counter").await?;

    assert_eq!(value, Some(2));

    Ok(())
}
