use crate::shared::{WorkerError, WorkerState, random_string, setup};
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(oxanus::Registry)]
struct BatchComponentRegistry(oxanus::ComponentRegistry<WorkerState, WorkerError>);

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "batch_test", concurrency = 1, registry = BatchComponentRegistry)]
struct BatchQueue;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BatchItemJob {
    value: usize,
}

#[derive(oxanus::Worker)]
#[oxanus(
    args = BatchItemJob,
    context = WorkerState,
    error = WorkerError,
    registry = BatchComponentRegistry
)]
struct BatchItemWorker;

impl BatchItemWorker {
    async fn process(
        &self,
        _job: &BatchItemJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(oxanus::BatchProcessor)]
#[oxanus(
    batch_size = 3,
    batch_linger_ms = 500,
    context = WorkerState,
    error = WorkerError,
    registry = BatchComponentRegistry
)]
struct TestBatchProcessor {
    items: Vec<BatchItemJob>,
}

impl TestBatchProcessor {
    async fn process_batch(&self, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL is not set");
        let cfg = deadpool_redis::Config::from_url(redis_url);
        let pool = cfg
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .expect("Failed to create pool");
        let mut redis = pool.get().await.expect("Failed to get redis conn");
        let batch_size = self.items.len() as i64;
        let _: () = redis
            .rpush("batch_test:sizes", batch_size)
            .await
            .expect("rpush failed");
        for item in &self.items {
            let _: () = redis
                .rpush("batch_test:values", item.value)
                .await
                .expect("rpush failed");
        }
        Ok(())
    }
}

#[tokio::test]
pub async fn test_batch() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let _: () = redis_conn.del("batch_test:sizes").await?;
    let _: () = redis_conn.del("batch_test:values").await?;

    let ctx = oxanus::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });

    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;

    let jobs_count = 7;

    let config =
        BatchComponentRegistry::build_config(&storage).exit_when_processed(jobs_count as u64);

    for i in 1..=jobs_count {
        storage
            .enqueue(BatchQueue, BatchItemJob { value: i })
            .await?;
    }

    assert_eq!(storage.enqueued_count(BatchQueue).await?, jobs_count);

    oxanus::run(config, ctx).await?;

    let sizes: Vec<i64> = redis_conn.lrange("batch_test:sizes", 0, -1).await?;
    let total: i64 = sizes.iter().sum();
    assert_eq!(total, jobs_count as i64);

    let mut sorted_sizes = sizes;
    sorted_sizes.sort();
    assert_eq!(sorted_sizes, vec![1, 3, 3]);

    let values: Vec<String> = redis_conn.lrange("batch_test:values", 0, -1).await?;
    assert_eq!(values.len(), jobs_count);

    assert_eq!(storage.enqueued_count(BatchQueue).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    Ok(())
}
