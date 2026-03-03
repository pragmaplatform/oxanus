use crate::shared::{WorkerError, WorkerState, random_string, setup};
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

#[derive(oxanus::Registry)]
struct BatchComponentRegistry(oxanus::ComponentRegistry<WorkerState, WorkerError>);

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "batch_test", concurrency = 1, registry = BatchComponentRegistry)]
struct BatchQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(context = WorkerState, error = WorkerError, registry = BatchComponentRegistry)]
struct BatchItem {
    value: usize,
}

#[derive(Debug, Serialize, Deserialize, oxanus::BatchProcessor)]
#[oxanus(
    batch_size = 3,
    batch_linger_ms = 500,
    context = WorkerState,
    error = WorkerError,
    registry = BatchComponentRegistry
)]
struct TestBatchProcessor {
    items: Vec<BatchItem>,
}

impl TestBatchProcessor {
    async fn process_batch(&self, ctx: &oxanus::Context<WorkerState>) -> Result<(), WorkerError> {
        let mut redis = ctx.ctx.redis.get().await?;
        let batch_size = self.items.len() as i64;
        let _: () = redis.rpush("batch_test:sizes", batch_size).await?;
        for item in &self.items {
            let _: () = redis.rpush("batch_test:values", item.value).await?;
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

    let ctx = oxanus::Context::value(WorkerState {
        redis: redis_pool.clone(),
    });

    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;

    let jobs_count = 7;

    let config =
        BatchComponentRegistry::build_config(&storage).exit_when_processed(jobs_count as u64);

    for i in 1..=jobs_count {
        storage.enqueue(BatchQueue, BatchItem { value: i }).await?;
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
