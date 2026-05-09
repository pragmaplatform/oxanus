use chrono::{DateTime, Utc};

use crate::{
    error::OxanaError,
    job_envelope::{JobEnvelope, JobId},
    metrics::{
        JobMetricsDetail, JobMetricsQuery, JobMetricsSnapshot, MetricIdentity,
        QueueLengthMetricsSnapshot,
    },
    queue::Queue,
    stats::{Process, QueueStats, Stats},
    storage_builder::StorageBuilder,
    storage_internal::StorageInternal,
    storage_types::QueueListOpts,
    worker::Job,
};

#[cfg(feature = "prometheus")]
use crate::prometheus::PrometheusMetrics;

/// Storage provides the main interface for job management in Oxana.
///
/// It handles all job operations including enqueueing, scheduling, and monitoring.
/// Storage instances are created using the [`Storage::builder()`] method.
///
/// # Examples
///
/// ```rust
/// use oxana::{Storage, Queue, Job};
///
/// async fn example() -> Result<(), oxana::OxanaError> {
///     let storage = Storage::builder().from_env()?.build()?;
///
///     // Enqueue a job
///     storage.enqueue(MyQueue, MyJob { data: "hello" }).await?;
///
///     // Schedule a job for later
///     storage.enqueue_in(MyQueue, MyJob { data: "delayed" }, 300).await?;
///
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct Storage {
    pub(crate) internal: StorageInternal,
}

impl Storage {
    /// Creates a new [`StorageBuilder`] for configuring and building a Storage instance.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxana::Storage;
    ///
    /// let builder = Storage::builder();
    /// let storage = builder.from_env()?.build()?;
    /// ```
    pub fn builder() -> StorageBuilder {
        StorageBuilder::new()
    }

    /// Enqueues a job to be processed immediately.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to enqueue the job to
    /// * `job` - The job to enqueue (must implement [`Job`])
    ///
    /// # Returns
    ///
    /// A [`JobId`] that can be used to track the job, or an [`OxanaError`] if the operation fails.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxana::{Storage, Queue, Job};
    ///
    /// async fn example(storage: &Storage) -> Result<(), oxana::OxanaError> {
    ///     let job_id = storage.enqueue(MyQueue, MyJob { data: "hello" }).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn enqueue<T: Job>(&self, queue: impl Queue, job: T) -> Result<JobId, OxanaError> {
        self.enqueue_in(queue, job, 0).await
    }

    /// Enqueues a job to be processed after a specified delay.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to enqueue the job to
    /// * `job` - The job to enqueue (must implement [`Job`])
    /// * `delay` - The delay in seconds before the job should be processed
    ///
    /// # Returns
    ///
    /// A [`JobId`] that can be used to track the job, or an [`OxanaError`] if the operation fails.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxana::{Storage, Queue, Job};
    ///
    /// async fn example(storage: &Storage) -> Result<(), oxana::OxanaError> {
    ///     // Schedule a job to run in 5 minutes
    ///     let job_id = storage.enqueue_in(MyQueue, MyJob { data: "delayed" }, 300).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn enqueue_in<T: Job>(
        &self,
        queue: impl Queue,
        job: T,
        delay: u64,
    ) -> Result<JobId, OxanaError> {
        let envelope = JobEnvelope::new(queue.key().clone(), job)?;

        tracing::trace!("Enqueuing job: {:?}", envelope);

        if delay > 0 {
            self.internal.enqueue_in(envelope, delay).await
        } else {
            self.internal.enqueue(envelope).await
        }
    }

    /// Schedules a job to run at a specific time.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to enqueue the job to
    /// * `job` - The job to enqueue (must implement [`Job`])
    /// * `time` - The UTC timestamp when the job should become available
    ///
    /// # Returns
    ///
    /// A [`JobId`] that can be used to track the job, or an [`OxanaError`] if the operation fails.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::{Duration, Utc};
    /// use oxana::{Storage, Queue, Job};
    ///
    /// async fn example(storage: &Storage) -> Result<(), oxana::OxanaError> {
    ///     let time = Utc::now() + Duration::minutes(5);
    ///     let job_id = storage.enqueue_at(MyQueue, MyJob { data: "scheduled" }, time).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn enqueue_at<T: Job>(
        &self,
        queue: impl Queue,
        job: T,
        time: DateTime<Utc>,
    ) -> Result<JobId, OxanaError> {
        let envelope = JobEnvelope::new_scheduled(queue.key().clone(), job, time)?;

        tracing::trace!("Scheduling job {:?} at {}", envelope, time);

        self.internal.enqueue_at(envelope).await
    }

    /// Returns the number of jobs currently enqueued in the specified queue.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to count jobs for
    ///
    /// # Returns
    ///
    /// The number of enqueued jobs, or an [`OxanaError`] if the operation fails.
    pub async fn enqueued_count(&self, queue: impl Queue) -> Result<usize, OxanaError> {
        self.internal.enqueued_count(&queue.key()).await
    }

    /// Returns the latency of the queue (The age of the oldest job in the queue).
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to get the latency for
    ///
    /// # Returns
    ///
    /// The latency of the queue in milliseconds, or an [`OxanaError`] if the operation fails.
    pub async fn latency_ms(&self, queue: impl Queue) -> Result<f64, OxanaError> {
        self.internal.latency_ms(&queue.key()).await
    }

    /// Returns the number of jobs that have failed and moved to the dead queue.
    ///
    /// # Returns
    ///
    /// The number of dead jobs, or an [`OxanaError`] if the operation fails.
    pub async fn dead_count(&self) -> Result<usize, OxanaError> {
        self.internal.dead_count().await
    }

    /// Returns the number of jobs that are currently being retried.
    ///
    /// # Returns
    ///
    /// The number of retrying jobs, or an [`OxanaError`] if the operation fails.
    pub async fn retries_count(&self) -> Result<usize, OxanaError> {
        self.internal.retries_count().await
    }

    /// Returns the number of jobs that are scheduled for future execution.
    ///
    /// # Returns
    ///
    /// The number of scheduled jobs, or an [`OxanaError`] if the operation fails.
    pub async fn scheduled_count(&self) -> Result<usize, OxanaError> {
        self.internal.scheduled_count().await
    }

    /// Returns the number of jobs that are currently enqueued or scheduled for future execution.
    ///
    /// # Returns
    ///
    /// The number of jobs, or an [`OxanaError`] if the operation fails.
    pub async fn jobs_count(&self) -> Result<usize, OxanaError> {
        self.internal.jobs_count().await
    }

    /// Deletes a job by its ID.
    ///
    /// Removes the job from both the job store and the processing queue.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the job to delete
    ///
    /// # Returns
    ///
    /// An [`OxanaError`] if the operation fails.
    pub async fn delete_job(&self, id: &JobId) -> Result<(), OxanaError> {
        self.internal.delete_job(id).await
    }

    /// Returns a job by its ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the job to return
    ///
    /// # Returns
    ///
    /// The job envelope when present, or [`None`] if the job no longer exists.
    pub async fn get_job(&self, id: &JobId) -> Result<Option<JobEnvelope>, OxanaError> {
        self.internal.get_job(id).await
    }

    /// Returns per-queue statistics for queues matching the given patterns.
    ///
    /// Each pattern is matched against queue names using the same glob syntax
    /// as Redis SCAN (e.g. `"email*"`, `"*"`).
    ///
    /// # Arguments
    ///
    /// * `patterns` - Glob patterns to match queue names against
    ///
    /// # Returns
    ///
    /// Statistics for matching queues, or an [`OxanaError`] if the operation fails.
    pub async fn stats_queues_for(&self, patterns: &[&str]) -> Result<Vec<QueueStats>, OxanaError> {
        self.internal.stats_queues_for(patterns).await
    }

    /// Returns per-queue statistics for all queues.
    ///
    /// # Returns
    ///
    /// Statistics for all queues, or an [`OxanaError`] if the operation fails.
    pub async fn stats_queues(&self) -> Result<Vec<QueueStats>, OxanaError> {
        self.internal.stats_queues().await
    }

    /// Returns the full stats including global aggregates, processes, and per-queue stats.
    ///
    /// # Returns
    ///
    /// The full stats, or an [`OxanaError`] if the operation fails.
    pub async fn stats(&self) -> Result<Stats, OxanaError> {
        self.internal.stats().await
    }

    /// Returns Sidekiq-style job execution metrics for all workers.
    ///
    /// Metrics are retained for up to 8 hours. The query defaults to 60 minutes
    /// and is clamped to 480 minutes.
    pub async fn job_metrics(
        &self,
        query: JobMetricsQuery,
    ) -> Result<JobMetricsSnapshot, OxanaError> {
        self.internal.job_metrics(query).await
    }

    /// Returns Sidekiq-style job execution metrics for a single worker.
    ///
    /// Job counters count every job. Execution counters, execution time, and
    /// histogram data count each worker execution once, so a batch worker
    /// contributes one execution sample for the whole batch.
    pub async fn job_metrics_for(
        &self,
        identity: &MetricIdentity,
        query: JobMetricsQuery,
    ) -> Result<JobMetricsDetail, OxanaError> {
        self.internal.job_metrics_for(identity, query).await
    }

    /// Returns per-minute queue length samples for active queues.
    ///
    /// Samples are recorded by workers during their periodic queue length refresh
    /// and retained for the same window as job execution metrics.
    pub async fn queue_length_metrics(
        &self,
        query: JobMetricsQuery,
    ) -> Result<QueueLengthMetricsSnapshot, OxanaError> {
        self.internal.queue_length_metrics(query).await
    }

    /// Returns the list of processes that are currently running.
    ///
    /// # Returns
    ///
    /// The list of processes, or an [`OxanaError`] if the operation fails.
    pub async fn processes(&self) -> Result<Vec<Process>, OxanaError> {
        self.internal.processes().await
    }

    /// Returns the namespace this storage instance is using.
    ///
    /// # Returns
    ///
    /// The namespace string.
    pub fn namespace(&self) -> &str {
        self.internal.namespace()
    }

    /// Returns a list of jobs currently enqueued in the specified queue.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to list jobs from
    /// * `opts` - Pagination options controlling count and offset
    ///
    /// # Returns
    ///
    /// A vector of [`JobEnvelope`]s, or an [`OxanaError`] if the operation fails.
    pub async fn list_queue_jobs(
        &self,
        queue: impl Queue,
        opts: &QueueListOpts,
    ) -> Result<Vec<JobEnvelope>, OxanaError> {
        self.internal.list_queue_jobs(&queue.key(), opts).await
    }

    /// Returns a list of dead jobs.
    ///
    /// # Arguments
    ///
    /// * `opts` - Pagination options controlling count and offset
    ///
    /// # Returns
    ///
    /// A vector of [`JobEnvelope`]s, or an [`OxanaError`] if the operation fails.
    pub async fn list_dead(&self, opts: &QueueListOpts) -> Result<Vec<JobEnvelope>, OxanaError> {
        self.internal.list_dead(opts).await
    }

    /// Returns a list of jobs pending retry.
    ///
    /// # Arguments
    ///
    /// * `opts` - Pagination options controlling count and offset
    ///
    /// # Returns
    ///
    /// A vector of [`JobEnvelope`]s, or an [`OxanaError`] if the operation fails.
    pub async fn list_retries(&self, opts: &QueueListOpts) -> Result<Vec<JobEnvelope>, OxanaError> {
        self.internal.list_retries(opts).await
    }

    /// Returns a list of jobs scheduled for future execution.
    ///
    /// # Arguments
    ///
    /// * `opts` - Pagination options controlling count and offset
    ///
    /// # Returns
    ///
    /// A vector of [`JobEnvelope`]s, or an [`OxanaError`] if the operation fails.
    pub async fn list_scheduled(
        &self,
        opts: &QueueListOpts,
    ) -> Result<Vec<JobEnvelope>, OxanaError> {
        self.internal.list_scheduled(opts).await
    }

    /// Enqueues a pre-built job envelope for immediate processing.
    ///
    /// Unlike [`enqueue`](Self::enqueue), this accepts a raw [`JobEnvelope`] directly,
    /// which is useful for re-enqueueing jobs from the dead queue or other sources
    /// where the original worker type is not available.
    ///
    /// # Arguments
    ///
    /// * `envelope` - The job envelope to enqueue
    ///
    /// # Returns
    ///
    /// The [`JobId`] of the enqueued job, or an [`OxanaError`] if the operation fails.
    pub async fn enqueue_envelope(&self, envelope: JobEnvelope) -> Result<JobId, OxanaError> {
        self.internal.enqueue(envelope).await
    }

    /// Removes all jobs from the specified queue.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to wipe
    ///
    /// # Returns
    ///
    /// An [`OxanaError`] if the operation fails.
    pub async fn wipe_queue(&self, queue: impl Queue) -> Result<(), OxanaError> {
        self.internal.wipe_queue(&queue.key()).await
    }

    /// Removes all jobs from the dead queue.
    ///
    /// # Returns
    ///
    /// An [`OxanaError`] if the operation fails.
    pub async fn wipe_dead(&self) -> Result<(), OxanaError> {
        self.internal.wipe_dead().await
    }

    /// Returns Prometheus metrics based on the current stats.
    ///
    /// # Returns
    ///
    /// A [`PrometheusMetrics`] instance containing all current metrics,
    /// or an [`OxanaError`] if fetching stats fails.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use oxana::Storage;
    ///
    /// async fn example(storage: &Storage) -> Result<(), oxana::OxanaError> {
    ///     let metrics = storage.metrics().await?;
    ///     let output = metrics.encode_to_string()?;
    ///     println!("{}", output);
    ///     Ok(())
    /// }
    /// ```
    #[cfg(feature = "prometheus")]
    pub async fn metrics(&self) -> Result<PrometheusMetrics, OxanaError> {
        let stats = self.stats().await?;
        Ok(PrometheusMetrics::from_stats(&stats))
    }
}
