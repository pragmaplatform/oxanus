use deadpool_redis::PoolConfig;
use serde::{Deserialize, Serialize};

fn main() {
    divan::main();
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerNoopJob {
    pub sleep_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Generic error: {0}")]
    GenericError(String),
}

#[derive(Debug, Clone)]
pub struct WorkerState {}

#[derive(oxana::Worker)]
#[oxana(context = WorkerState, error = ServiceError, registry = None)]
pub struct WorkerNoop;

impl oxana::Job for WorkerNoopJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<WorkerNoop>()
    }
}

impl WorkerNoop {
    async fn process(
        &self,
        job: WorkerNoopJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), ServiceError> {
        tokio::time::sleep(std::time::Duration::from_millis(job.sleep_ms)).await;
        Ok(())
    }
}

#[derive(Serialize)]
pub struct QueueOne;

impl oxana::Queue for QueueOne {
    fn to_config() -> oxana::QueueConfig {
        oxana::QueueConfig {
            kind: oxana::QueueKind::Static {
                key: "one".to_string(),
            },
            concurrency: oxana::QueueConcurrency::Fixed(1),
            throttle: None,
        }
    }
}

const JOBS_COUNT: u64 = 1000;
const CONCURRENCY: &[usize] = &[1, 2, 4, 8, 12, 16, 512];

macro_rules! bench_jobs {
    ($name:ident, $sleep_ms:expr) => {
        #[divan::bench(args = CONCURRENCY, sample_size = 1, sample_count = 1)]
        fn $name(bencher: divan::Bencher, n: usize) {
            let rt = &tokio::runtime::Runtime::new().unwrap();
            let storage = build_storage();
            rt.block_on(async { setup(&storage, JOBS_COUNT, $sleep_ms).await.unwrap() });

            bencher.bench(|| {
                rt.block_on(async {
                    execute(storage.clone(), n, JOBS_COUNT).await.unwrap();
                })
            });
        }
    };
}

bench_jobs!(run_1000_jobs_taking_0_ms, 0);
bench_jobs!(run_1000_jobs_taking_1_ms, 1);
bench_jobs!(run_1000_jobs_taking_2_ms, 2);
bench_jobs!(run_1000_jobs_taking_10_ms, 10);

async fn setup(
    storage: &oxana::Storage,
    jobs_count: u64,
    sleep_ms: u64,
) -> Result<(), oxana::OxanaError> {
    for _ in 0..jobs_count {
        storage
            .enqueue(QueueOne, WorkerNoopJob { sleep_ms })
            .await?;
    }

    Ok(())
}

async fn execute(
    storage: oxana::Storage,
    concurrency: usize,
    jobs_count: u64,
) -> Result<(), oxana::OxanaError> {
    let storage = storage
        .register_queue_with_concurrency::<QueueOne>(concurrency)
        .register_worker::<WorkerNoop, WorkerNoopJob, WorkerState>()
        .exit_when_processed(jobs_count);
    let ctx = oxana::ContextValue::new(WorkerState {});

    let stats = storage.clone().run(ctx).await?;

    assert_eq!(stats.processed, jobs_count);
    assert_eq!(stats.succeeded, jobs_count);
    assert_eq!(stats.failed, 0);

    Ok(())
}

fn redis_pool() -> deadpool_redis::Pool {
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL is not set");
    let mut cfg = deadpool_redis::Config::from_url(redis_url);
    cfg.pool = Some(PoolConfig {
        max_size: 512,
        ..Default::default()
    });
    cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("Failed to create Redis pool")
}

fn build_storage() -> oxana::Storage {
    dotenvy::from_filename(".env.test").ok();
    oxana::Storage::builder()
        .build_from_pool(redis_pool())
        .expect("Failed to build storage")
}
