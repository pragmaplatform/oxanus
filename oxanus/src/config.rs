use std::collections::HashSet;
use std::pin::Pin;
use tokio_util::sync::CancellationToken;

use crate::Storage;
use crate::queue::{Queue, QueueConfig};
use crate::worker::Worker;
use crate::worker_registry::{WorkerConfig, WorkerRegistry};

pub struct Config<DT, ET> {
    pub(crate) registry: WorkerRegistry<DT, ET>,
    pub(crate) queues: HashSet<QueueConfig>,
    pub(crate) exit_when_processed: Option<u64>,
    pub(crate) shutdown_signal:
        Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send + Sync>>,
    pub(crate) shutdown_timeout: std::time::Duration,
    pub(crate) cancel_token: CancellationToken,
    pub storage: Storage,
}

impl<DT, ET> Config<DT, ET> {
    pub fn new(storage: &Storage) -> Self {
        Self {
            registry: WorkerRegistry::new(),
            queues: HashSet::new(),
            exit_when_processed: None,
            shutdown_signal: Box::pin(default_shutdown_signal()),
            shutdown_timeout: std::time::Duration::from_secs(180),
            cancel_token: CancellationToken::new(),
            storage: storage.clone(),
        }
    }

    pub fn register_queue<Q>(mut self) -> Self
    where
        Q: Queue,
    {
        self.register_queue_with(Q::to_config());
        self
    }

    pub fn register_queue_with(&mut self, config: QueueConfig) {
        self.queues.insert(config);
    }

    pub fn register_queue_with_concurrency<Q>(mut self, concurrency: usize) -> Self
    where
        Q: Queue,
    {
        let mut config = Q::to_config();
        config.concurrency = concurrency;
        self.register_queue_with(config);
        self
    }

    pub fn register_worker<W>(mut self) -> Self
    where
        W: Worker<Context = DT, Error = ET> + serde::de::DeserializeOwned + 'static,
    {
        if let (Some(schedule), Some(queue_config)) = (W::cron_schedule(), W::cron_queue_config()) {
            let key = queue_config
                .static_key()
                .expect("Statically defined cron workers can only use static queues");
            self.register_queue_with(queue_config);
            self.registry.register_cron::<W>(&schedule, key);
        } else {
            self.registry.register::<W>();
        }
        self
    }

    /// Register a cron worker with a dynamic queue
    pub fn register_cron_worker<W>(mut self, queue: impl Queue) -> Self
    where
        W: Worker<Context = DT, Error = ET> + serde::de::DeserializeOwned + 'static,
    {
        self.register_queue_with(queue.config());
        let schedule = W::cron_schedule().expect("Cron Worker must have cron_schedule defined");
        self.registry.register_cron::<W>(&schedule, queue.key());
        self
    }

    pub fn register_worker_with(&mut self, config: WorkerConfig<DT, ET>) {
        self.registry.register_worker_with(config);
    }

    pub fn exit_when_processed(mut self, processed: u64) -> Self {
        self.exit_when_processed = Some(processed);
        self
    }

    pub fn with_graceful_shutdown(
        mut self,
        fut: impl Future<Output = Result<(), std::io::Error>> + Send + Sync + 'static,
    ) -> Self {
        self.shutdown_signal = Box::pin(fut);
        self
    }

    pub fn consume_shutdown_signal(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send + Sync + 'static>> {
        let mut shutdown_signal = no_signal();
        std::mem::swap(&mut self.shutdown_signal, &mut shutdown_signal);
        shutdown_signal
    }

    pub fn has_registered_queue<Q: Queue>(&self) -> bool {
        self.queues.contains(&Q::to_config())
    }

    pub fn has_registered_worker<W>(&self) -> bool
    where
        W: Worker<Context = DT, Error = ET>,
    {
        self.registry.has_registered::<W>()
    }

    pub fn has_registered_cron_worker<W>(&self) -> bool
    where
        W: Worker<Context = DT, Error = ET>,
    {
        self.registry.has_registered_cron::<W>()
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
