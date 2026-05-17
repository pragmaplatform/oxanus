use crate::shared::*;
use serde::Serialize;
use testresult::TestResult;

#[derive(Serialize)]
struct QueueDynamic(i32);

impl oxana::Queue for QueueDynamic {
    fn to_config() -> oxana::QueueConfig {
        oxana::QueueConfig::as_dynamic("dynamic")
    }
}

#[tokio::test]
pub async fn test_dynamic() -> TestResult {
    let redis_pool = setup();
    let ctx = oxana::ContextValue::new(());
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?
        .register_queue::<QueueDynamic>()
        .register_worker::<WorkerNoop, WorkerNoopJob, ()>()
        .exit_when_processed(2);

    storage.enqueue(QueueDynamic(1), WorkerNoopJob {}).await?;
    storage.enqueue(QueueDynamic(2), WorkerNoopJob {}).await?;

    assert_eq!(storage.enqueued_count(QueueDynamic(1)).await?, 1);
    assert_eq!(storage.enqueued_count(QueueDynamic(2)).await?, 1);
    assert_eq!(storage.enqueued_count(QueueDynamic(3)).await?, 0);

    let stats = storage.clone().run(ctx).await?;

    assert_eq!(stats.processed, 2);
    assert_eq!(storage.dead_count().await?, 0);
    assert_eq!(storage.enqueued_count(QueueDynamic(1)).await?, 0);
    assert_eq!(storage.enqueued_count(QueueDynamic(2)).await?, 0);
    assert_eq!(storage.enqueued_count(QueueDynamic(3)).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    Ok(())
}
