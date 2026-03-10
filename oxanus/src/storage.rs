use chrono::{DateTime, Utc};

use crate::{
    error::OxanusError,
    job_envelope::{JobEnvelope, JobId},
    queue::Queue,
    stats::{Process, Stats},
    storage_builder::StorageBuilder,
    storage_internal::StorageInternal,
    storage_types::QueueListOpts,
    worker::Job,
};

#[cfg(feature = "prometheus")]
use crate::prometheus::PrometheusMetrics;

/// Storage provides the main interface for job management in Oxanus.
///
/// It handles all job operations including enqueueing, scheduling, and monitoring.
/// Storage instances are created using the [`Storage::builder()`] method.
///
/// # Examples
///
/// ```rust,ignore
/// use oxanus::Storage;
///
/// async fn example() -> Result<(), oxanus::OxanusError> {
///     let storage = Storage::builder().build_from_env()?;
///
///     storage.enqueue(MyQueue, MyJob { data: "hello" }).await?;
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
    /// ```rust,ignore
    /// use oxanus::Storage;
    ///
    /// let storage = Storage::builder().build_from_env()?;
    /// ```
    pub fn builder() -> StorageBuilder {
        StorageBuilder::new()
    }

    /// Enqueues a job to be processed immediately.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to enqueue the job to
    /// * `job` - The job to enqueue
    ///
    /// # Returns
    ///
    /// A [`JobId`] that can be used to track the job, or an [`OxanusError`] if the operation fails.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use oxanus::Storage;
    ///
    /// async fn example(storage: &Storage) -> Result<(), oxanus::OxanusError> {
    ///     let job_id = storage.enqueue(MyQueue, MyJob { data: "hello".into() }).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn enqueue(&self, queue: impl Queue, job: impl Job) -> Result<JobId, OxanusError> {
        self.enqueue_in(queue, job, 0).await
    }

    /// Enqueues a job to be processed after a specified delay.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to enqueue the job to
    /// * `job` - The job to enqueue
    /// * `delay` - The delay in seconds before the job should be processed
    ///
    /// # Returns
    ///
    /// A [`JobId`] that can be used to track the job, or an [`OxanusError`] if the operation fails.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use oxanus::Storage;
    ///
    /// async fn example(storage: &Storage) -> Result<(), oxanus::OxanusError> {
    ///     // Schedule a job to run in 5 minutes
    ///     let job_id = storage.enqueue_in(MyQueue, MyJob { data: "delayed".into() }, 300).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn enqueue_in(
        &self,
        queue: impl Queue,
        job: impl Job,
        delay: u64,
    ) -> Result<JobId, OxanusError> {
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
    /// * `job` - The job to enqueue
    /// * `time` - The UTC timestamp when the job should become available
    ///
    /// # Returns
    ///
    /// A [`JobId`] that can be used to track the job, or an [`OxanusError`] if the operation fails.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use chrono::{Duration, Utc};
    /// use oxanus::Storage;
    ///
    /// async fn example(storage: &Storage) -> Result<(), oxanus::OxanusError> {
    ///     let time = Utc::now() + Duration::minutes(5);
    ///     let job_id = storage.enqueue_at(MyQueue, MyJob { data: "scheduled".into() }, time).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn enqueue_at(
        &self,
        queue: impl Queue,
        job: impl Job,
        time: DateTime<Utc>,
    ) -> Result<JobId, OxanusError> {
        let envelope = JobEnvelope::new(queue.key().clone(), job)?;

        tracing::trace!("Scheduling job {:?} at {}", envelope, time);

        self.internal.enqueue_at(envelope, time).await
    }

    /// Returns the number of jobs currently enqueued in the specified queue.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to count jobs for
    pub async fn enqueued_count(&self, queue: impl Queue) -> Result<usize, OxanusError> {
        self.internal.enqueued_count(&queue.key()).await
    }

    /// Returns the latency of the queue (the age of the oldest job in the queue).
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to get the latency for
    ///
    /// # Returns
    ///
    /// The latency of the queue in milliseconds, or an [`OxanusError`] if the operation fails.
    pub async fn latency_ms(&self, queue: impl Queue) -> Result<f64, OxanusError> {
        self.internal.latency_ms(&queue.key()).await
    }

    /// Returns the number of jobs that have failed and moved to the dead queue.
    pub async fn dead_count(&self) -> Result<usize, OxanusError> {
        self.internal.dead_count().await
    }

    /// Returns the number of jobs that are currently being retried.
    pub async fn retries_count(&self) -> Result<usize, OxanusError> {
        self.internal.retries_count().await
    }

    /// Returns the number of jobs that are scheduled for future execution.
    pub async fn scheduled_count(&self) -> Result<usize, OxanusError> {
        self.internal.scheduled_count().await
    }

    /// Returns the number of jobs that are currently enqueued or scheduled for future execution.
    pub async fn jobs_count(&self) -> Result<usize, OxanusError> {
        self.internal.jobs_count().await
    }

    /// Deletes a job by its ID.
    ///
    /// Removes the job from both the job store and the processing queue.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the job to delete
    pub async fn delete_job(&self, id: &JobId) -> Result<(), OxanusError> {
        self.internal.delete_job(id).await
    }

    /// Returns the stats for all queues.
    pub async fn stats(&self) -> Result<Stats, OxanusError> {
        self.internal.stats().await
    }

    /// Returns the list of processes that are currently running.
    pub async fn processes(&self) -> Result<Vec<Process>, OxanusError> {
        self.internal.processes().await
    }

    /// Returns the namespace this storage instance is using.
    pub fn namespace(&self) -> &str {
        self.internal.namespace()
    }

    /// Returns a list of jobs currently enqueued in the specified queue.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to list jobs from
    /// * `opts` - Pagination options controlling count and offset
    pub async fn list_queue_jobs(
        &self,
        queue: impl Queue,
        opts: &QueueListOpts,
    ) -> Result<Vec<JobEnvelope>, OxanusError> {
        self.internal.list_queue_jobs(&queue.key(), opts).await
    }

    /// Returns a list of dead jobs.
    ///
    /// # Arguments
    ///
    /// * `opts` - Pagination options controlling count and offset
    pub async fn list_dead(&self, opts: &QueueListOpts) -> Result<Vec<JobEnvelope>, OxanusError> {
        self.internal.list_dead(opts).await
    }

    /// Returns a list of jobs pending retry.
    ///
    /// # Arguments
    ///
    /// * `opts` - Pagination options controlling count and offset
    pub async fn list_retries(
        &self,
        opts: &QueueListOpts,
    ) -> Result<Vec<JobEnvelope>, OxanusError> {
        self.internal.list_retries(opts).await
    }

    /// Returns a list of jobs scheduled for future execution.
    ///
    /// # Arguments
    ///
    /// * `opts` - Pagination options controlling count and offset
    pub async fn list_scheduled(
        &self,
        opts: &QueueListOpts,
    ) -> Result<Vec<JobEnvelope>, OxanusError> {
        self.internal.list_scheduled(opts).await
    }

    /// Removes all jobs from the specified queue.
    ///
    /// # Arguments
    ///
    /// * `queue` - The queue to wipe
    pub async fn wipe_queue(&self, queue: impl Queue) -> Result<(), OxanusError> {
        self.internal.wipe_queue(&queue.key()).await
    }

    /// Returns Prometheus metrics based on the current stats.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use oxanus::Storage;
    ///
    /// async fn example(storage: &Storage) -> Result<(), oxanus::OxanusError> {
    ///     let metrics = storage.metrics().await?;
    ///     let output = metrics.encode_to_string()?;
    ///     println!("{}", output);
    ///     Ok(())
    /// }
    /// ```
    #[cfg(feature = "prometheus")]
    pub async fn metrics(&self) -> Result<PrometheusMetrics, OxanusError> {
        let stats = self.stats().await?;
        Ok(PrometheusMetrics::from_stats(&stats))
    }
}
