use crate::shared::*;
use deadpool_redis::redis::AsyncCommands;
use std::time::Duration;
use testresult::TestResult;

#[tokio::test]
pub async fn test_standard() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;

    let ctx = oxana::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });

    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;
    let config = oxana::Config::new(&storage)
        .register_queue::<QueueOne>()
        .register_worker::<WorkerRedisSet, WorkerRedisSetJob>()
        .exit_when_processed(1);

    let random_key = uuid::Uuid::new_v4().to_string();
    let random_value = uuid::Uuid::new_v4().to_string();

    storage
        .enqueue(
            QueueOne,
            WorkerRedisSetJob {
                key: random_key.clone(),
                value: random_value.clone(),
            },
        )
        .await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 1);

    oxana::run(config, ctx).await?;

    let value: Option<String> = redis_conn.get(random_key).await?;

    assert_eq!(value, Some(random_value));
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    Ok(())
}

#[tokio::test]
pub async fn test_paused_queue_resumes_after_runtime_config_update() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;

    let ctx = oxana::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });

    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;
    storage
        .set_queue_state(QueueOne, oxana::QueueState::Paused)
        .await?;

    let config = oxana::Config::new(&storage)
        .register_queue::<QueueOne>()
        .register_worker::<WorkerRedisSet, WorkerRedisSetJob>()
        .exit_when_processed(1);

    let random_key = uuid::Uuid::new_v4().to_string();
    let random_value = uuid::Uuid::new_v4().to_string();

    storage
        .enqueue(
            QueueOne,
            WorkerRedisSetJob {
                key: random_key.clone(),
                value: random_value.clone(),
            },
        )
        .await?;

    let handle = tokio::spawn(async move { oxana::run(config, ctx).await });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let value: Option<String> = redis_conn.get(&random_key).await?;
    assert_eq!(value, None);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 1);

    storage.unpause_queue(QueueOne).await?;

    tokio::time::timeout(Duration::from_secs(5), handle).await???;
    let value: Option<String> = redis_conn.get(random_key).await?;

    assert_eq!(value, Some(random_value));
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);

    Ok(())
}

#[tokio::test]
pub async fn test_fixed_queue_rejects_runtime_concurrency_override() -> TestResult {
    let redis_pool = setup();
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;

    let error = storage
        .set_queue_concurrency(QueueOne, 2)
        .await
        .expect_err("fixed queues should reject runtime concurrency overrides");

    assert!(matches!(error, oxana::OxanaError::ConfigError(_)));

    Ok(())
}
