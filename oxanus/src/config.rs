use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::Storage;
use crate::queue::{Queue, QueueConfig};
use crate::storage_types::{Catalog, CronWorkerInfo, QueueInfo, QueueThrottleInfo, WorkerInfo};
use crate::worker::{FromContext, Job, Worker};
use crate::worker_registry::{self, WorkerConfig, WorkerConfigKind, WorkerRegistry};

type RetryDelayOverrideFn<ET> = dyn Fn(&ET, u32, u64) -> Option<u64> + Send + Sync;

pub struct Config<DT, ET> {
    pub(crate) registry: WorkerRegistry<DT, ET>,
    pub(crate) queues: HashSet<QueueConfig>,
    pub(crate) exit_when_processed: Option<u64>,
    pub(crate) shutdown_signal:
        Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send + Sync>>,
    pub(crate) shutdown_timeout: std::time::Duration,
    pub(crate) cancel_token: CancellationToken,
    pub(crate) retry_delay_override: Option<Arc<RetryDelayOverrideFn<ET>>>,
    pub storage: Storage,
}

impl<DT, ET> Config<DT, ET> {
    /// Creates a new configuration with default settings.
    pub fn new(storage: &Storage) -> Self {
        Self {
            registry: WorkerRegistry::new(),
            queues: HashSet::new(),
            exit_when_processed: None,
            shutdown_signal: Box::pin(default_shutdown_signal()),
            shutdown_timeout: std::time::Duration::from_secs(180),
            cancel_token: CancellationToken::new(),
            retry_delay_override: None,
            storage: storage.clone(),
        }
    }

    /// Registers a queue by its type.
    pub fn register_queue<Q>(mut self) -> Self
    where
        Q: Queue,
    {
        self.register_queue_with(Q::to_config());
        self
    }

    /// Registers a queue from a [`QueueConfig`].
    pub fn register_queue_with(&mut self, config: QueueConfig) {
        self.queues.insert(config);
    }

    /// Registers a queue by its type with a custom concurrency limit.
    pub fn register_queue_with_concurrency<Q>(mut self, concurrency: usize) -> Self
    where
        Q: Queue,
    {
        let mut config = Q::to_config();
        config.concurrency = concurrency;
        self.register_queue_with(config);
        self
    }

    /// Registers a worker and its associated job type.
    pub fn register_worker<W, A>(mut self) -> Self
    where
        W: Worker<A, Error = ET> + FromContext<DT> + 'static,
        A: Job + serde::de::DeserializeOwned + Send + Sync + 'static,
        DT: Clone + Send + Sync + 'static,
        ET: std::error::Error + Send + Sync + 'static,
    {
        let name = A::worker_name().to_string();
        let factory = worker_registry::job_factory::<W, A, DT, ET>;
        let kind = <W as Worker<A>>::to_config();

        if let WorkerConfigKind::Cron { .. } = &kind {
            assert!(
                serde_json::from_value::<A>(serde_json::json!({})).is_ok(),
                "{name}: Cron job args must be deserializable from empty JSON `{{}}`. \
                 Use `#[serde(default)]` on all fields or define the args struct with no fields."
            );

            if let Some(queue_config) = <W as Worker<A>>::cron_queue_config() {
                self.register_queue_with(queue_config);
            }
        }

        self.registry.register_worker_with(WorkerConfig {
            name,
            factory,
            kind,
        });
        self
    }

    /// Registers a worker from a [`WorkerConfig`].
    pub fn register_worker_with(&mut self, config: WorkerConfig<DT, ET>) {
        self.registry.register_worker_with(config);
    }

    /// Stops processing after the given number of jobs have been processed. Useful for testing.
    pub fn exit_when_processed(mut self, processed: u64) -> Self {
        self.exit_when_processed = Some(processed);
        self
    }

    /// Sets a future that triggers graceful shutdown when it completes.
    /// Defaults to listening for SIGTERM/SIGINT on Unix and Ctrl+C on Windows.
    pub fn with_graceful_shutdown(
        mut self,
        fut: impl Future<Output = Result<(), std::io::Error>> + Send + Sync + 'static,
    ) -> Self {
        self.shutdown_signal = Box::pin(fut);
        self
    }

    /// Sets a global callback to override the retry delay when a job fails.
    ///
    /// The callback receives `(error, retry_count, default_delay)` and returns
    /// `Some(seconds)` to override or `None` to use the worker's default.
    pub fn with_retry_delay_override(
        mut self,
        f: impl Fn(&ET, u32, u64) -> Option<u64> + Send + Sync + 'static,
    ) -> Self {
        self.retry_delay_override = Some(Arc::new(f));
        self
    }

    pub fn consume_shutdown_signal(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send + Sync + 'static>> {
        let mut shutdown_signal = no_signal();
        std::mem::swap(&mut self.shutdown_signal, &mut shutdown_signal);
        shutdown_signal
    }

    /// Returns `true` if the given queue type has been registered.
    pub fn has_registered_queue<Q: Queue>(&self) -> bool {
        self.queues.contains(&Q::to_config())
    }

    /// Returns `true` if a worker with the given name has been registered.
    pub fn has_registered_worker(&self, name: &str) -> bool {
        self.registry.has_registered(name)
    }

    /// Returns `true` if a worker of the given type has been registered.
    pub fn has_registered_worker_type<W: 'static>(&self) -> bool {
        self.registry.has_registered(std::any::type_name::<W>())
    }

    /// Returns `true` if a cron worker with the given name has been registered.
    pub fn has_registered_cron_worker(&self, name: &str) -> bool {
        self.registry.has_registered_cron(name)
    }

    /// Returns `true` if a cron worker of the given type has been registered.
    pub fn has_registered_cron_worker_type<W: 'static>(&self) -> bool {
        self.registry
            .has_registered_cron(std::any::type_name::<W>())
    }

    /// Returns a catalog of all registered workers.
    pub fn catalog(&self) -> Catalog {
        let mut cron_workers: Vec<CronWorkerInfo> = self
            .registry
            .schedules
            .iter()
            .map(|(name, cron_job)| CronWorkerInfo {
                name: name.clone(),
                schedule: cron_job.schedule.clone(),
                queue_key: cron_job.queue_key.clone(),
            })
            .collect();
        cron_workers.sort_by(|a, b| a.name.cmp(&b.name));

        let mut workers: Vec<WorkerInfo> = self
            .registry
            .worker_names()
            .into_iter()
            .filter(|name| !self.registry.schedules.contains_key(*name))
            .map(|name| WorkerInfo {
                name: name.to_string(),
            })
            .collect();
        workers.sort_by(|a, b| a.name.cmp(&b.name));

        let mut queues: Vec<QueueInfo> = self
            .queues
            .iter()
            .map(|q| {
                let (key, dynamic) = match &q.kind {
                    crate::queue::QueueKind::Static { key } => (key.clone(), false),
                    crate::queue::QueueKind::Dynamic { prefix, .. } => (prefix.clone(), true),
                };
                QueueInfo {
                    key,
                    dynamic,
                    concurrency: q.concurrency,
                    throttle: q.throttle.as_ref().map(|t| QueueThrottleInfo {
                        window_ms: t.window_ms,
                        limit: t.limit,
                    }),
                }
            })
            .collect();
        queues.sort_by(|a, b| a.key.cmp(&b.key));

        Catalog {
            workers,
            cron_workers,
            queues,
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn default_shutdown_signal() -> Result<(), std::io::Error> {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    tokio::select! {
        _ = ctrl_c => Ok(()),
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(target_os = "windows")]
async fn default_shutdown_signal() -> Result<(), std::io::Error> {
    tokio::signal::ctrl_c().await
}

fn no_signal() -> Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send + Sync + 'static>>
{
    Box::pin(async move { Ok(()) })
}
