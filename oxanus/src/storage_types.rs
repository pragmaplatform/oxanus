use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Options for listing jobs in a queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueListOpts {
    /// Maximum number of jobs to return.
    pub count: usize,
    /// Number of jobs to skip from the start.
    pub offset: usize,
}

/// Catalog of all registered workers.
#[derive(Debug, Clone)]
pub struct WorkersCatalog {
    /// Regular (non-cron) workers.
    pub workers: Vec<WorkerInfo>,
    /// Cron workers with schedule information.
    pub cron_workers: Vec<CronWorkerInfo>,
}

/// Information about a registered worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    /// The worker name (Rust type path).
    pub name: String,
}

/// Information about a registered cron worker.
#[derive(Debug, Clone)]
pub struct CronWorkerInfo {
    /// The worker name (Rust type path).
    pub name: String,
    /// The cron schedule expression.
    pub schedule: cron::Schedule,
    /// The queue key this worker runs on.
    pub queue_key: String,
    /// The next scheduled run time, if any.
    pub next_run: Option<DateTime<Utc>>,
}
