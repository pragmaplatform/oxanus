use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

use crate::shared::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerUniqueSkipJob {
    pub id: i32,
    pub key: String,
    pub value: i32,
}

pub struct WorkerUniqueSkip {
    state: WorkerState,
}

impl oxana::Job for WorkerUniqueSkipJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<WorkerUniqueSkip>()
    }
    fn unique_id(&self) -> Option<String> {
        Some(format!("unique:{}", self.id))
    }
    fn on_conflict(&self) -> oxana::JobConflictStrategy {
        oxana::JobConflictStrategy::Skip
    }
}

impl oxana::FromContext<WorkerState> for WorkerUniqueSkip {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

#[async_trait::async_trait]
impl oxana::Worker<WorkerUniqueSkipJob> for WorkerUniqueSkip {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        jobs: Vec<oxana::BatchItem<WorkerUniqueSkipJob>>,
    ) -> Result<(), WorkerError> {
        let mut redis = self.state.redis.get().await?;
        for item in jobs {
            let job = item.job;
            let _: () = redis.set_ex(&job.key, job.value.to_string(), 3).await?;
        }
        Ok(())
    }

    fn retry_delay(&self, _job: &WorkerUniqueSkipJob, _retries: u32) -> u64 {
        0
    }
    fn max_retries(&self, _job: &WorkerUniqueSkipJob) -> u32 {
        0
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerUniqueReplaceJob {
    pub id: i32,
    pub key: String,
    pub value: i32,
}

pub struct WorkerUniqueReplace {
    state: WorkerState,
}

impl oxana::Job for WorkerUniqueReplaceJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<WorkerUniqueReplace>()
    }
    fn unique_id(&self) -> Option<String> {
        Some(format!("unique:{}", self.id))
    }
    fn on_conflict(&self) -> oxana::JobConflictStrategy {
        oxana::JobConflictStrategy::Replace
    }
}

impl oxana::FromContext<WorkerState> for WorkerUniqueReplace {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

#[async_trait::async_trait]
impl oxana::Worker<WorkerUniqueReplaceJob> for WorkerUniqueReplace {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        jobs: Vec<oxana::BatchItem<WorkerUniqueReplaceJob>>,
    ) -> Result<(), WorkerError> {
        let mut redis = self.state.redis.get().await?;
        for item in jobs {
            let job = item.job;
            let _: () = redis.set_ex(&job.key, job.value.to_string(), 3).await?;
        }
        Ok(())
    }

    fn retry_delay(&self, _job: &WorkerUniqueReplaceJob, _retries: u32) -> u64 {
        0
    }
    fn max_retries(&self, _job: &WorkerUniqueReplaceJob) -> u32 {
        0
    }
}

#[tokio::test]
pub async fn test_unique_skip() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let ctx = oxana::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?
        .register_queue::<QueueOne>()
        .register_worker::<WorkerUniqueSkip, WorkerUniqueSkipJob, WorkerState>()
        .exit_when_processed(2);
    let key1 = random_string();
    let key2 = random_string();

    storage
        .enqueue(
            QueueOne,
            WorkerUniqueSkipJob {
                id: 1,
                key: key1.clone(),
                value: 1,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            WorkerUniqueSkipJob {
                id: 1,
                key: key1.clone(),
                value: 2,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            WorkerUniqueSkipJob {
                id: 2,
                key: key2.clone(),
                value: 3,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            WorkerUniqueSkipJob {
                id: 2,
                key: key2.clone(),
                value: 4,
            },
        )
        .await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 2);

    storage.clone().run(ctx).await?;

    assert_eq!(storage.dead_count().await?, 0);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    let value: Option<i32> = redis_conn.get(key1).await?;
    assert_eq!(value, Some(1));
    let value: Option<i32> = redis_conn.get(key2).await?;
    assert_eq!(value, Some(3));

    Ok(())
}

#[tokio::test]
pub async fn test_unique_replace() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let ctx = oxana::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?
        .register_queue::<QueueOne>()
        .register_worker::<WorkerUniqueReplace, WorkerUniqueReplaceJob, WorkerState>()
        .exit_when_processed(2);

    let key1 = random_string();
    let key2 = random_string();

    storage
        .enqueue(
            QueueOne,
            WorkerUniqueReplaceJob {
                id: 1,
                key: key1.clone(),
                value: 1,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            WorkerUniqueReplaceJob {
                id: 1,
                key: key1.clone(),
                value: 2,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            WorkerUniqueReplaceJob {
                id: 2,
                key: key2.clone(),
                value: 3,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            WorkerUniqueReplaceJob {
                id: 2,
                key: key2.clone(),
                value: 4,
            },
        )
        .await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 2);

    storage.clone().run(ctx).await?;

    assert_eq!(storage.dead_count().await?, 0);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    let value: Option<i32> = redis_conn.get(key1).await?;
    assert_eq!(value, Some(2));
    let value: Option<i32> = redis_conn.get(key2).await?;
    assert_eq!(value, Some(4));

    Ok(())
}
