use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

use crate::shared::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerRedisSetWithRetryJob {
    pub key: String,
    pub value_first: String,
    pub value_second: String,
}

pub struct WorkerRedisSetWithRetry {
    state: WorkerState,
}

impl oxana::Job for WorkerRedisSetWithRetryJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<WorkerRedisSetWithRetry>()
    }
}

impl oxana::FromContext<WorkerState> for WorkerRedisSetWithRetry {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

#[async_trait::async_trait]
impl oxana::Worker<WorkerRedisSetWithRetryJob> for WorkerRedisSetWithRetry {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        jobs: Vec<oxana::BatchItem<WorkerRedisSetWithRetryJob>>,
    ) -> Result<(), WorkerError> {
        let mut redis = self.state.redis.get().await?;
        for item in jobs {
            let job = item.job;
            let value: Option<String> = redis.get(&job.key).await?;
            if value.is_some() {
                let _: () = redis.set_ex(&job.key, job.value_second, 3).await?;
                continue;
            }
            let _: () = redis.set_ex(&job.key, job.value_first, 3).await?;
            return Err(WorkerError::Generic("Key not set".to_string()));
        }
        Ok(())
    }

    fn retry_delay(&self, _job: &WorkerRedisSetWithRetryJob, _retries: u32) -> u64 {
        0
    }
    fn max_retries(&self, _job: &WorkerRedisSetWithRetryJob) -> u32 {
        1
    }
}

#[tokio::test]
pub async fn test_retry() -> TestResult {
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
        .register_worker::<WorkerRedisSetWithRetry, WorkerRedisSetWithRetryJob>()
        .exit_when_processed(2);

    let random_key = uuid::Uuid::new_v4().to_string();
    let random_value_first = uuid::Uuid::new_v4().to_string();
    let random_value_second = uuid::Uuid::new_v4().to_string();

    storage
        .enqueue(
            QueueOne,
            WorkerRedisSetWithRetryJob {
                key: random_key.clone(),
                value_first: random_value_first.clone(),
                value_second: random_value_second.clone(),
            },
        )
        .await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 1);

    oxana::run(config, ctx).await?;

    let value: Option<String> = redis_conn.get(random_key).await?;

    assert_eq!(value, Some(random_value_second));
    assert_eq!(storage.dead_count().await?, 0);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerStateResumeDefaultJob {
    pub attempts_key: String,
    pub observations_key: String,
    pub read_mode: WorkerStateReadMode,
}

pub struct WorkerStateResumeDefault {
    state: WorkerState,
}

impl oxana::Job for WorkerStateResumeDefaultJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<WorkerStateResumeDefault>()
    }
}

impl oxana::FromContext<WorkerState> for WorkerStateResumeDefault {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

#[async_trait::async_trait]
impl oxana::Worker<WorkerStateResumeDefaultJob> for WorkerStateResumeDefault {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        jobs: Vec<oxana::BatchItem<WorkerStateResumeDefaultJob>>,
    ) -> Result<(), WorkerError> {
        for item in jobs {
            record_state_retry_attempt(
                &self.state,
                &item.ctx,
                &item.job.attempts_key,
                &item.job.observations_key,
                item.job.read_mode,
            )
            .await?;
        }
        Ok(())
    }

    fn retry_delay(&self, _job: &WorkerStateResumeDefaultJob, _retries: u32) -> u64 {
        0
    }

    fn max_retries(&self, _job: &WorkerStateResumeDefaultJob) -> u32 {
        1
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerStateNoResumeJob {
    pub attempts_key: String,
    pub observations_key: String,
    pub read_mode: WorkerStateReadMode,
}

pub struct WorkerStateNoResume {
    state: WorkerState,
}

impl oxana::Job for WorkerStateNoResumeJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<WorkerStateNoResume>()
    }

    fn should_resume() -> bool {
        false
    }
}

impl oxana::FromContext<WorkerState> for WorkerStateNoResume {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

#[async_trait::async_trait]
impl oxana::Worker<WorkerStateNoResumeJob> for WorkerStateNoResume {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        jobs: Vec<oxana::BatchItem<WorkerStateNoResumeJob>>,
    ) -> Result<(), WorkerError> {
        for item in jobs {
            record_state_retry_attempt(
                &self.state,
                &item.ctx,
                &item.job.attempts_key,
                &item.job.observations_key,
                item.job.read_mode,
            )
            .await?;
        }
        Ok(())
    }

    fn retry_delay(&self, _job: &WorkerStateNoResumeJob, _retries: u32) -> u64 {
        0
    }

    fn max_retries(&self, _job: &WorkerStateNoResumeJob) -> u32 {
        1
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkerStateReadMode {
    State,
    Progress,
}

async fn record_state_retry_attempt(
    state: &WorkerState,
    ctx: &oxana::JobContext,
    attempts_key: &str,
    observations_key: &str,
    read_mode: WorkerStateReadMode,
) -> Result<(), WorkerError> {
    let observed = match read_mode {
        WorkerStateReadMode::State => {
            let seen_state = ctx
                .state
                .get::<i64>()
                .await
                .map_err(|e| WorkerError::Generic(e.to_string()))?;
            seen_state.map_or_else(|| "none".to_string(), |value| value.to_string())
        }
        WorkerStateReadMode::Progress => {
            let seen_progress = ctx
                .state
                .progress()
                .await
                .map_err(|e| WorkerError::Generic(e.to_string()))?;
            seen_progress.map_or_else(|| "none".to_string(), |value| value.cursor.to_string())
        }
    };

    let mut redis = state.redis.get().await?;
    let _: usize = redis.rpush(observations_key, observed).await?;
    let attempt: i64 = redis.incr(attempts_key, 1).await?;

    match read_mode {
        WorkerStateReadMode::State => ctx.state.update(attempt).await,
        WorkerStateReadMode::Progress => ctx.state.update_progress((attempt, 2)).await,
    }
    .map_err(|e| WorkerError::Generic(e.to_string()))?;

    if attempt == 1 {
        return Err(WorkerError::Generic("retry once".to_string()));
    }

    Ok(())
}

async fn run_state_retry_test<W, A>(
    build_job: impl FnOnce(String, String) -> A,
    expected_observations: &[&str],
) -> TestResult
where
    W: oxana::Worker<A, Error = WorkerError> + oxana::FromContext<WorkerState> + 'static,
    A: oxana::Job + serde::de::DeserializeOwned + Send + 'static,
{
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
        .register_worker::<W, A>()
        .exit_when_processed(2);

    let attempts_key = uuid::Uuid::new_v4().to_string();
    let observations_key = uuid::Uuid::new_v4().to_string();

    storage
        .enqueue(QueueOne, build_job(attempts_key, observations_key.clone()))
        .await?;

    oxana::run(config, ctx).await?;

    let observations: Vec<String> = redis_conn.lrange(observations_key, 0, -1).await?;
    let expected_observations = expected_observations
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    assert_eq!(observations, expected_observations);
    assert_eq!(storage.dead_count().await?, 0);

    Ok(())
}

#[tokio::test]
pub async fn test_retry_resumes_state_by_default() -> TestResult {
    run_state_retry_test::<WorkerStateResumeDefault, WorkerStateResumeDefaultJob>(
        |attempts_key, observations_key| WorkerStateResumeDefaultJob {
            attempts_key,
            observations_key,
            read_mode: WorkerStateReadMode::State,
        },
        &["none", "1"],
    )
    .await
}

#[tokio::test]
pub async fn test_retry_resumes_progress_by_default() -> TestResult {
    run_state_retry_test::<WorkerStateResumeDefault, WorkerStateResumeDefaultJob>(
        |attempts_key, observations_key| WorkerStateResumeDefaultJob {
            attempts_key,
            observations_key,
            read_mode: WorkerStateReadMode::Progress,
        },
        &["none", "1"],
    )
    .await
}

#[tokio::test]
pub async fn test_retry_resets_state_when_resume_is_false() -> TestResult {
    run_state_retry_test::<WorkerStateNoResume, WorkerStateNoResumeJob>(
        |attempts_key, observations_key| WorkerStateNoResumeJob {
            attempts_key,
            observations_key,
            read_mode: WorkerStateReadMode::State,
        },
        &["none", "none"],
    )
    .await
}

#[tokio::test]
pub async fn test_retry_resets_progress_when_resume_is_false() -> TestResult {
    run_state_retry_test::<WorkerStateNoResume, WorkerStateNoResumeJob>(
        |attempts_key, observations_key| WorkerStateNoResumeJob {
            attempts_key,
            observations_key,
            read_mode: WorkerStateReadMode::Progress,
        },
        &["none", "none"],
    )
    .await
}
