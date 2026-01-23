//! Stats types for Oxanus job queue monitoring.

use serde::{Deserialize, Serialize};

use crate::job_envelope::JobEnvelope;

/// Overall statistics for the Oxanus job queue system.
#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    /// Global aggregate statistics.
    pub global: StatsGlobal,
    /// List of active processes.
    pub processes: Vec<Process>,
    /// Jobs currently being processed.
    pub processing: Vec<StatsProcessing>,
    /// Per-queue statistics.
    pub queues: Vec<QueueStats>,
}

/// Global aggregate statistics.
#[derive(Debug, Clone, Serialize)]
pub struct StatsGlobal {
    /// Total number of jobs (enqueued + scheduled).
    pub jobs: usize,
    /// Number of jobs currently enqueued.
    pub enqueued: usize,
    /// Total number of jobs processed.
    pub processed: i64,
    /// Number of dead jobs.
    pub dead: usize,
    /// Number of scheduled jobs.
    pub scheduled: usize,
    /// Number of jobs in retry queue.
    pub retries: usize,
    /// Maximum latency across all queues in seconds.
    pub latency_s_max: f64,
}

/// Information about a job currently being processed.
#[derive(Debug, Clone, Serialize)]
pub struct StatsProcessing {
    /// The process ID handling the job.
    pub process_id: String,
    /// The job envelope being processed.
    pub job_envelope: JobEnvelope,
}

/// Statistics for a specific queue.
#[derive(Debug, Clone, Serialize)]
pub struct QueueStats {
    /// The queue key/name.
    pub key: String,

    /// Number of jobs currently enqueued.
    pub enqueued: usize,
    /// Total number of jobs processed.
    pub processed: i64,
    /// Total number of jobs succeeded.
    pub succeeded: i64,
    /// Total number of jobs panicked.
    pub panicked: i64,
    /// Total number of jobs failed.
    pub failed: i64,
    /// Current latency in seconds.
    pub latency_s: f64,

    /// Dynamic sub-queue statistics (if any).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub queues: Vec<DynamicQueueStats>,
}

/// Statistics for a dynamic sub-queue.
#[derive(Debug, Clone, Serialize)]
pub struct DynamicQueueStats {
    /// The dynamic queue suffix.
    pub suffix: String,

    /// Number of jobs currently enqueued.
    pub enqueued: usize,
    /// Total number of jobs processed.
    pub processed: i64,
    /// Total number of jobs succeeded.
    pub succeeded: i64,
    /// Total number of jobs panicked.
    pub panicked: i64,
    /// Total number of jobs failed.
    pub failed: i64,
    /// Current latency in seconds.
    pub latency_s: f64,
}

/// Information about an Oxanus worker process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    /// The hostname where the process is running.
    pub hostname: String,
    /// The process ID.
    pub pid: u32,
    /// Last heartbeat timestamp (Unix timestamp).
    pub heartbeat_at: i64,
    /// Process start timestamp (Unix timestamp).
    pub started_at: i64,
}

impl Process {
    /// Returns a unique identifier for the process.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}-{}", self.hostname, self.pid)
    }
}
