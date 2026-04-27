use deadpool_redis::redis::{self, AsyncCommands};
use std::{
    collections::{HashMap, HashSet},
    num::NonZero,
    sync::Arc,
    sync::atomic::{AtomicU32, Ordering},
};
use tokio_util::sync::CancellationToken;

use crate::{
    OxanusError,
    job_envelope::{JobConflictStrategy, JobEnvelope, JobId},
    result_collector::{JobResult, JobResultKind},
    stats::{DynamicQueueStats, Process, QueueStats, Stats, StatsGlobal, StatsProcessing},
    storage_keys::StorageKeys,
    storage_types::QueueListOpts,
    worker_registry::CronJob,
};

const JOB_EXPIRE_TIME: i64 = 7 * 24 * 3600; // 7 days
const RESURRECT_THRESHOLD_SECS: i64 = 5;
const MAX_CONSECUTIVE_REDIS_FAILURES: u32 = 30;

#[derive(Clone)]
pub(crate) struct StorageInternal {
    pool: deadpool_redis::Pool,
    keys: StorageKeys,
    started_at: i64,
    consecutive_redis_failures: Arc<AtomicU32>,
}

enum JobEnqueueAction {
    Default,
    Skip,
    Replace,
}

impl StorageInternal {
    pub fn new(pool: deadpool_redis::Pool, namespace: Option<String>) -> Self {
        let keys = StorageKeys::new(namespace.unwrap_or_default());
        Self {
            pool,
            keys,
            started_at: chrono::Utc::now().timestamp(),
            consecutive_redis_failures: Arc::new(AtomicU32::new(0)),
        }
    }

    fn record_redis_success(&self) {
        if self.consecutive_redis_failures.load(Ordering::Relaxed) != 0 {
            self.consecutive_redis_failures.store(0, Ordering::Relaxed);
        }
    }

    fn record_redis_failure(&self, err: &OxanusError) -> bool {
        let count = self
            .consecutive_redis_failures
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        tracing::warn!(error = %err, consecutive_failures = count, "Transient Redis error");
        count >= MAX_CONSECUTIVE_REDIS_FAILURES
    }

    /// Wraps a Redis operation result with resilience tracking.
    /// Returns `Ok(Some(value))` on success, `Ok(None)` on transient failure,
    /// or `Err(e)` if the consecutive failure threshold has been exceeded.
    pub(crate) fn track_redis_result<T>(
        &self,
        result: Result<T, OxanusError>,
    ) -> Result<Option<T>, OxanusError> {
        match result {
            Ok(val) => {
                self.record_redis_success();
                Ok(Some(val))
            }
            Err(e) => {
                if self.record_redis_failure(&e) {
                    Err(e)
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub fn namespace(&self) -> &str {
        &self.keys.namespace
    }

    pub async fn pool(&self) -> Result<deadpool_redis::Pool, OxanusError> {
        Ok(self.pool.clone())
    }

    pub async fn connection(&self) -> Result<deadpool_redis::Connection, OxanusError> {
        self.pool
            .get()
            .await
            .map_err(OxanusError::DeadpoolRedisPoolError)
    }

    pub async fn queue_keys(&self, pattern: &str) -> Result<HashSet<String>, OxanusError> {
        let mut conn = self.connection().await?;
        let keys: Vec<String> = (*conn).keys(self.namespace_queue(pattern)).await?;
        Ok(keys.into_iter().collect())
    }

    pub async fn queues(&self, pattern: &str) -> Result<Vec<String>, OxanusError> {
        let queue_keys = self.queue_keys(pattern).await?;
        // remove namespace prefix from beginning of each key
        let queues = queue_keys
            .into_iter()
            .map(|key| key.replace(&format!("{}:", &self.keys.queue_prefix), ""))
            .collect();
        Ok(queues)
    }

    pub async fn enqueue(&self, envelope: JobEnvelope) -> Result<JobId, OxanusError> {
        let mut redis = self.connection().await?;
        self.enqueue_w_conn(&mut redis, envelope).await
    }

    async fn enqueue_w_conn(
        &self,
        redis: &mut deadpool_redis::Connection,
        envelope: JobEnvelope,
    ) -> Result<JobId, OxanusError> {
        match self.job_enqueue_action(redis, &envelope).await? {
            JobEnqueueAction::Skip => {
                tracing::warn!("Unique job {} already exists, skipping", envelope.id);

                return Ok(envelope.id);
            }
            JobEnqueueAction::Replace => {
                tracing::warn!("Unique job {} already exists, replacing", envelope.id);

                let _: () = deadpool_redis::redis::pipe()
                    .hset(
                        &self.keys.jobs,
                        &envelope.id,
                        serde_json::to_string(&envelope)?,
                    )
                    .query_async(&mut *redis)
                    .await?;
            }
            JobEnqueueAction::Default => {
                let _: () = deadpool_redis::redis::pipe()
                    .hset(
                        &self.keys.jobs,
                        &envelope.id,
                        serde_json::to_string(&envelope)?,
                    )
                    .lpush(self.namespace_queue(&envelope.queue), &envelope.id)
                    .query_async(&mut *redis)
                    .await?;
            }
        }

        Ok(envelope.id)
    }

    async fn job_enqueue_action(
        &self,
        redis: &mut deadpool_redis::Connection,
        envelope: &JobEnvelope,
    ) -> Result<JobEnqueueAction, OxanusError> {
        if !envelope.meta.unique {
            return Ok(JobEnqueueAction::Default);
        }

        let exists: bool = redis.hexists(&self.keys.jobs, &envelope.id).await?;

        if exists {
            match envelope.meta.on_conflict {
                Some(JobConflictStrategy::Skip) | None => Ok(JobEnqueueAction::Skip),
                Some(JobConflictStrategy::Replace) => Ok(JobEnqueueAction::Replace),
            }
        } else {
            Ok(JobEnqueueAction::Default)
        }
    }

    pub async fn enqueue_in(
        &self,
        envelope: JobEnvelope,
        delay_s: u64,
    ) -> Result<JobId, OxanusError> {
        if delay_s == 0 {
            self.enqueue(envelope).await
        } else {
            let time = chrono::Utc::now() + chrono::Duration::seconds(delay_s as i64);
            self.enqueue_at(envelope.with_scheduled_at(time)).await
        }
    }

    pub async fn enqueue_at(&self, envelope: JobEnvelope) -> Result<JobId, OxanusError> {
        if envelope.meta.scheduled_at <= chrono::Utc::now().timestamp_micros() {
            return self.enqueue(envelope).await;
        }

        let mut redis = self.connection().await?;

        match self.job_enqueue_action(&mut redis, &envelope).await? {
            JobEnqueueAction::Skip => {
                tracing::warn!("Unique job {} already exists, skipping", envelope.id);

                return Ok(envelope.id);
            }
            JobEnqueueAction::Replace => {
                tracing::warn!("Unique job {} already exists, replacing", envelope.id);

                let _: () = redis::pipe()
                    .hset(
                        &self.keys.jobs,
                        &envelope.id,
                        serde_json::to_string(&envelope)?,
                    )
                    .zadd(
                        &self.keys.schedule,
                        &envelope.id,
                        envelope.meta.scheduled_at,
                    )
                    .query_async(&mut redis)
                    .await?;
            }
            JobEnqueueAction::Default => {
                let _: () = redis::pipe()
                    .hset(
                        &self.keys.jobs,
                        &envelope.id,
                        serde_json::to_string(&envelope)?,
                    )
                    .zadd(
                        &self.keys.schedule,
                        &envelope.id,
                        envelope.meta.scheduled_at,
                    )
                    .query_async(&mut redis)
                    .await?;
            }
        }

        Ok(envelope.id)
    }

    pub async fn retry_in(
        &self,
        job_id: JobId,
        delay_s: u64,
        error: String,
    ) -> Result<(), OxanusError> {
        let updated_envelope = self
            .get_job(&job_id)
            .await?
            .ok_or(OxanusError::JobNotFound)?
            .with_retries_incremented(error);

        let now = chrono::Utc::now().timestamp_micros() as u64;

        let mut redis = self.connection().await?;
        let _: () = redis::pipe()
            .hset(
                &self.keys.jobs,
                &updated_envelope.id,
                serde_json::to_string(&updated_envelope)?,
            )
            // .hexpire(
            //     &self.keys.jobs,
            //     JOB_EXPIRE_TIME,
            //     redis::ExpireOption::NONE,
            //     &updated_envelope.id,
            // )
            .zadd(
                &self.keys.retry,
                updated_envelope.id,
                now + delay_s * 1_000_000,
            )
            .query_async(&mut redis)
            .await?;
        Ok(())
    }

    pub async fn blocking_dequeue(
        &self,
        queue: &str,
        timeout: f64,
    ) -> Result<Option<JobId>, OxanusError> {
        let mut redis = self.connection().await?;
        let job_id: Option<String> = redis
            .blmove(
                self.namespace_queue(queue),
                self.current_processing_queue(),
                redis::Direction::Right,
                redis::Direction::Left,
                timeout,
            )
            .await?;
        Ok(job_id)
    }

    pub async fn dequeue(&self, queue: &str) -> Result<Option<JobId>, OxanusError> {
        let mut redis = self.connection().await?;
        let job_id: Option<JobId> = redis
            .lmove(
                self.namespace_queue(queue),
                self.current_processing_queue(),
                redis::Direction::Right,
                redis::Direction::Left,
            )
            .await?;
        Ok(job_id)
    }

    pub async fn get_job(&self, id: &JobId) -> Result<Option<JobEnvelope>, OxanusError> {
        let mut redis = self.connection().await?;
        self.get_job_w_conn(&mut redis, id).await
    }

    async fn get_job_w_conn(
        &self,
        redis: &mut deadpool_redis::Connection,
        id: &JobId,
    ) -> Result<Option<JobEnvelope>, OxanusError> {
        let envelope: Option<String> = redis.hget(&self.keys.jobs, id).await?;
        match envelope {
            Some(envelope) => Ok(Some(serde_json::from_str(&envelope)?)),
            None => Ok(None),
        }
    }

    pub async fn update_job(&self, envelope: &JobEnvelope) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        let _: () = redis::pipe()
            .hset(
                &self.keys.jobs,
                &envelope.id,
                serde_json::to_string(envelope)?,
            )
            .query_async(&mut redis)
            .await?;
        Ok(())
    }

    pub async fn update_state(
        &self,
        id: &JobId,
        state: serde_json::Value,
    ) -> Result<JobEnvelope, OxanusError> {
        let mut envelope = match self.get_job(id).await? {
            Some(envelope) => envelope,
            None => return Err(OxanusError::JobNotFound),
        };
        envelope.meta.state = Some(state);
        self.update_job(&envelope).await?;
        Ok(envelope)
    }

    pub async fn set_started_at(&self, envelope: &mut JobEnvelope) -> Result<(), OxanusError> {
        envelope.meta.started_at = Some(chrono::Utc::now().timestamp_micros());
        self.update_job(envelope).await?;
        Ok(())
    }

    pub async fn delete_job(&self, id: &JobId) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        let _: () = redis::pipe()
            .hdel(&self.keys.jobs, id)
            .lrem(self.current_processing_queue(), 1, id)
            .query_async(&mut redis)
            .await?;
        Ok(())
    }

    pub async fn get_many(&self, ids: &[JobId]) -> Result<Vec<JobEnvelope>, OxanusError> {
        let mut redis = self.connection().await?;
        let mut cmd = redis::cmd("HMGET");
        cmd.arg(&self.keys.jobs);
        cmd.arg(ids);
        let envelopes_str: Vec<Option<String>> = cmd.query_async(&mut redis).await?;
        let mut envelopes: Vec<JobEnvelope> = vec![];
        for envelope_str in envelopes_str.into_iter().flatten() {
            envelopes.push(serde_json::from_str(&envelope_str)?);
        }
        Ok(envelopes)
    }

    pub async fn kill(&self, envelope: &JobEnvelope, error: String) -> Result<(), OxanusError> {
        let envelope = envelope.clone().with_error(error);
        let mut redis = self.connection().await?;
        let _: () = redis::pipe()
            .lrem(self.current_processing_queue(), 1, &envelope.id)
            .hdel(&self.keys.jobs, &envelope.id)
            .lpush(&self.keys.dead, &serde_json::to_string(&envelope)?)
            .ltrim(&self.keys.dead, 0, 999)
            .query_async(&mut redis)
            .await?;
        Ok(())
    }

    pub async fn finish_with_success(&self, envelope: &JobEnvelope) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        let _: () = redis::pipe()
            .hdel(&self.keys.jobs, &envelope.id)
            .lrem(self.current_processing_queue(), 1, &envelope.id)
            .query_async(&mut redis)
            .await?;
        Ok(())
    }

    pub async fn finish_with_failure(&self, envelope: &JobEnvelope) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        let _: () = redis::pipe()
            .lrem(self.current_processing_queue(), 1, &envelope.id)
            .query_async(&mut redis)
            .await?;
        Ok(())
    }

    pub async fn enqueue_scheduled(&self, schedule_queue: &str) -> Result<usize, OxanusError> {
        let now = chrono::Utc::now().timestamp_micros();
        let mut redis = self.connection().await?;
        let job_ids: Vec<String> = redis.zrangebyscore(schedule_queue, 0, now).await?;

        if job_ids.is_empty() {
            return Ok(0);
        }

        let mut claim_pipe = redis::pipe();
        for job_id in &job_ids {
            claim_pipe.zrem(schedule_queue, job_id);
        }
        let claimed: Vec<u32> = claim_pipe.query_async(&mut redis).await?;

        let claimed_job_ids: Vec<String> = job_ids
            .into_iter()
            .zip(claimed.iter())
            .filter(|(_, removed)| **removed > 0)
            .map(|(id, _)| id)
            .collect();

        if claimed_job_ids.is_empty() {
            return Ok(0);
        }

        let envelopes = self.get_many(&claimed_job_ids).await?;
        let envelopes_count = envelopes.len();

        let mut enqueue_pipe = redis::pipe();
        let mut map: HashMap<&str, Vec<&str>> = HashMap::new();

        for envelope in envelopes.iter() {
            map.entry(&envelope.queue)
                .or_default()
                .push(envelope.id.as_str());
        }

        for (queue, job_ids) in map {
            enqueue_pipe.lpush(self.namespace_queue(queue), job_ids);
        }

        let _: () = enqueue_pipe.query_async(&mut redis).await?;

        Ok(envelopes_count)
    }

    pub async fn list_queue_jobs(
        &self,
        queue: &str,
        opts: &QueueListOpts,
    ) -> Result<Vec<JobEnvelope>, OxanusError> {
        let mut redis = self.connection().await?;
        let start = opts.offset as isize;
        let stop = (opts.offset + opts.count).saturating_sub(1) as isize;
        let job_ids: Vec<JobId> = (*redis)
            .lrange(self.namespace_queue(queue), start, stop)
            .await?;

        if job_ids.is_empty() {
            return Ok(vec![]);
        }

        self.get_many(&job_ids).await
    }

    pub async fn wipe_queue(&self, queue: &str) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        let queue_key = self.namespace_queue(queue);

        let job_ids: Vec<JobId> = (*redis).lrange(&queue_key, 0, -1).await?;

        if !job_ids.is_empty() {
            let mut pipe = redis::pipe();
            pipe.hdel(&self.keys.jobs, &job_ids);
            pipe.del(&queue_key);
            let _: () = pipe.query_async(&mut redis).await?;
        } else {
            let _: () = (*redis).del(&queue_key).await?;
        }

        Ok(())
    }

    pub async fn list_dead(&self, opts: &QueueListOpts) -> Result<Vec<JobEnvelope>, OxanusError> {
        let mut redis = self.connection().await?;
        let start = opts.offset as isize;
        let stop = (opts.offset + opts.count).saturating_sub(1) as isize;
        let entries: Vec<String> = (*redis).lrange(&self.keys.dead, start, stop).await?;

        let jobs: Vec<JobEnvelope> = entries
            .into_iter()
            .filter_map(|s| serde_json::from_str::<JobEnvelope>(&s).ok())
            .collect();

        Ok(jobs)
    }

    pub async fn list_retries(
        &self,
        opts: &QueueListOpts,
    ) -> Result<Vec<JobEnvelope>, OxanusError> {
        let mut redis = self.connection().await?;
        let start = opts.offset as isize;
        let stop = (opts.offset + opts.count).saturating_sub(1) as isize;
        let job_ids: Vec<JobId> = (*redis).zrange(&self.keys.retry, start, stop).await?;

        if job_ids.is_empty() {
            return Ok(vec![]);
        }

        self.get_many(&job_ids).await
    }

    pub async fn list_scheduled(
        &self,
        opts: &QueueListOpts,
    ) -> Result<Vec<JobEnvelope>, OxanusError> {
        let mut redis = self.connection().await?;
        let start = opts.offset as isize;
        let stop = (opts.offset + opts.count).saturating_sub(1) as isize;
        let job_ids: Vec<JobId> = (*redis).zrange(&self.keys.schedule, start, stop).await?;

        if job_ids.is_empty() {
            return Ok(vec![]);
        }

        self.get_many(&job_ids).await
    }

    pub async fn enqueued_count(&self, queue: &str) -> Result<usize, OxanusError> {
        let mut redis = self.connection().await?;
        self.enqueued_count_w_conn(&mut redis, queue).await
    }

    async fn enqueued_count_w_conn(
        &self,
        redis: &mut deadpool_redis::Connection,
        queue: &str,
    ) -> Result<usize, OxanusError> {
        let count: i64 = (*redis).llen(self.namespace_queue(queue)).await?;
        Ok(count as usize)
    }

    async fn latency_s_w_conn(
        &self,
        redis: &mut deadpool_redis::Connection,
        queue: &str,
    ) -> Result<f64, OxanusError> {
        self.latency_micros_w_conn(redis, queue)
            .await
            .map(|latency| latency / 1_000_000.0)
    }

    pub async fn latency_ms(&self, queue: &str) -> Result<f64, OxanusError> {
        self.latency_micros(queue)
            .await
            .map(|latency| latency / 1_000.0)
    }

    pub async fn latency_micros(&self, queue: &str) -> Result<f64, OxanusError> {
        let mut redis = self.connection().await?;
        self.latency_micros_w_conn(&mut redis, queue).await
    }

    async fn latency_micros_w_conn(
        &self,
        redis: &mut deadpool_redis::Connection,
        queue: &str,
    ) -> Result<f64, OxanusError> {
        let result: Option<String> = (*redis).lindex(self.namespace_queue(queue), -1).await?;
        match result.as_ref() {
            Some(job_id) => {
                let envelope = self.get_job_w_conn(redis, job_id).await?;
                Ok(envelope.map_or(0.0, |envelope| {
                    let now = chrono::Utc::now().timestamp_micros();
                    (now - envelope.meta.effective_scheduled_at_micros()) as f64
                }))
            }
            None => Ok(0.0),
        }
    }

    pub async fn dead_count(&self) -> Result<usize, OxanusError> {
        let mut redis = self.connection().await?;
        let count: i64 = (*redis).llen(&self.keys.dead).await?;
        Ok(count as usize)
    }

    pub async fn retries_count(&self) -> Result<usize, OxanusError> {
        let mut redis = self.connection().await?;
        let count: i64 = (*redis).zcard(&self.keys.retry).await?;
        Ok(count as usize)
    }

    pub async fn scheduled_count(&self) -> Result<usize, OxanusError> {
        let mut redis = self.connection().await?;
        let count: i64 = (*redis).zcard(&self.keys.schedule).await?;
        Ok(count as usize)
    }

    pub async fn jobs_count(&self) -> Result<usize, OxanusError> {
        let mut redis = self.connection().await?;
        let count: i64 = (*redis).hlen(&self.keys.jobs).await?;
        Ok(count as usize)
    }

    pub async fn stats_queues_for(
        &self,
        patterns: &[&str],
    ) -> Result<Vec<QueueStats>, OxanusError> {
        let mut redis = self.connection().await?;
        let mut matched_queues = Vec::new();
        for pattern in patterns {
            matched_queues.extend(self.queues(pattern).await?);
        }
        matched_queues.sort();
        matched_queues.dedup();

        self.build_queue_stats(&mut redis, &matched_queues, true)
            .await
    }

    pub async fn stats_queues(&self) -> Result<Vec<QueueStats>, OxanusError> {
        let mut redis = self.connection().await?;
        let queues = self.queues("*").await?;
        self.build_queue_stats(&mut redis, &queues, false).await
    }

    pub async fn stats(&self) -> Result<Stats, OxanusError> {
        let mut redis = self.connection().await?;

        let queues = self.queues("*").await?;
        let values = self.build_queue_stats(&mut redis, &queues, false).await?;

        let mut processed_count_total = 0;
        let mut enqueued_count_total = 0;
        let mut failed_count_total = 0;
        let mut latency_s_max: f64 = 0.0;

        for value in &values {
            if value.latency_s > latency_s_max {
                latency_s_max = value.latency_s;
            }
            for dq in &value.queues {
                if dq.latency_s > latency_s_max {
                    latency_s_max = dq.latency_s;
                }
            }
            processed_count_total += value.processed;
            enqueued_count_total += value.enqueued;
            failed_count_total += value.failed;
        }

        let processes = self.processes().await?;

        let mut processing = vec![];

        for process in processes.iter() {
            let processing_queue = self.processing_queue(&process.id());
            let job_ids: Vec<String> = (*redis).lrange(&processing_queue, 0, -1).await?;

            for job_id in job_ids {
                if let Some(envelope) = self.get_job(&job_id).await? {
                    processing.push(StatsProcessing {
                        process_id: process.id(),
                        job_envelope: envelope,
                    });
                }
            }
        }

        Ok(Stats {
            global: StatsGlobal {
                jobs: self.jobs_count().await?,
                enqueued: enqueued_count_total,
                processed: processed_count_total,
                failed: failed_count_total,
                dead: self.dead_count().await?,
                scheduled: self.scheduled_count().await?,
                retries: self.retries_count().await?,
                latency_s_max,
            },
            processing,
            processes,
            queues: values,
        })
    }

    async fn build_queue_stats(
        &self,
        redis: &mut deadpool_redis::Connection,
        queues: &[String],
        filter: bool,
    ) -> Result<Vec<QueueStats>, OxanusError> {
        let list: HashMap<String, i64> = (*redis).hgetall(&self.keys.stats).await?;

        let mut map = HashMap::new();
        let mut queue_values: Vec<(String, String, i64)> = Vec::new();

        for queue in queues {
            queue_values.push((queue.clone(), "processed".to_string(), 0));
        }

        for (key, value) in list {
            let parts: Vec<&str> = key.rsplitn(2, ':').collect();
            let mut parts_iter = parts.into_iter();
            let stat_key = match parts_iter.next() {
                Some(stat_key) => stat_key,
                None => continue,
            };
            let queue_full_key = match parts_iter.next() {
                Some(queue_key) => queue_key,
                None => continue,
            };

            if filter {
                let base_key = queue_full_key
                    .split_once('#')
                    .map_or(queue_full_key, |(base, _)| base);
                let matches = queues.iter().any(|q| {
                    let q_base = q.split_once('#').map_or(q.as_str(), |(base, _)| base);
                    q_base == base_key || q == queue_full_key
                });
                if !matches {
                    continue;
                }
            }

            queue_values.push((queue_full_key.to_string(), stat_key.to_string(), value));
        }

        for (queue_full_key, stat_key, value) in queue_values {
            let queue_key_parts: Vec<&str> = queue_full_key.splitn(2, '#').collect();
            let mut queue_key_parts_iter = queue_key_parts.into_iter();

            let queue_key = match queue_key_parts_iter.next() {
                Some(queue_key) => queue_key,
                None => continue,
            };

            let queue_dynamic_key = queue_key_parts_iter.next();

            let queue_stats = map
                .entry(queue_key.to_string())
                .or_insert_with(|| QueueStats {
                    key: queue_key.to_string(),
                    enqueued: 0,
                    processed: 0,
                    succeeded: 0,
                    panicked: 0,
                    failed: 0,
                    latency_s: 0.0,
                    queues: vec![],
                });

            if let Some(queue_dynamic_key) = queue_dynamic_key {
                if !queue_stats
                    .queues
                    .iter_mut()
                    .any(|q| q.suffix == queue_dynamic_key)
                {
                    queue_stats.queues.push(DynamicQueueStats {
                        suffix: queue_dynamic_key.to_string(),
                        enqueued: 0,
                        processed: 0,
                        succeeded: 0,
                        panicked: 0,
                        failed: 0,
                        latency_s: 0.0,
                    });
                }

                if let Some(existing) = queue_stats
                    .queues
                    .iter_mut()
                    .find(|q| q.suffix == queue_dynamic_key)
                {
                    match stat_key.as_str() {
                        "processed" => existing.processed += value,
                        "succeeded" => existing.succeeded += value,
                        "panicked" => existing.panicked += value,
                        "failed" => existing.failed += value,
                        _ => {}
                    }
                }
            }

            match stat_key.as_str() {
                "processed" => queue_stats.processed += value,
                "succeeded" => queue_stats.succeeded += value,
                "panicked" => queue_stats.panicked += value,
                "failed" => queue_stats.failed += value,
                _ => {}
            }
        }

        let mut values: Vec<QueueStats> = map.into_values().collect();

        for value in values.iter_mut() {
            if value.queues.is_empty() {
                value.enqueued = self.enqueued_count_w_conn(redis, &value.key).await?;
                value.latency_s = self.latency_s_w_conn(redis, &value.key).await?;
            } else {
                for dynamic_queue in value.queues.iter_mut() {
                    let dynamic_queue_key = format!("{}#{}", value.key, dynamic_queue.suffix);
                    let enqueued = self
                        .enqueued_count_w_conn(redis, &dynamic_queue_key)
                        .await?;
                    let latency_s = self.latency_s_w_conn(redis, &dynamic_queue_key).await?;

                    dynamic_queue.enqueued = enqueued;
                    dynamic_queue.latency_s = latency_s;

                    if value.latency_s < latency_s {
                        value.latency_s = latency_s;
                    }
                    value.enqueued += enqueued;
                }
            }

            value.queues.sort_by(|a, b| a.suffix.cmp(&b.suffix));
        }

        values.sort_by(|a, b| a.key.cmp(&b.key));

        Ok(values)
    }

    pub async fn update_stats(&self, result: JobResult) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        let queue = result.envelope.queue.clone();

        let processed_key = format!("{queue}:processed");
        let status_key = match result.kind {
            JobResultKind::Success => format!("{queue}:succeeded"),
            JobResultKind::Panicked => format!("{queue}:panicked"),
            JobResultKind::Failed => format!("{queue}:failed"),
        };

        let _: () = redis::pipe()
            .hincr(&self.keys.stats, processed_key, 1)
            .hincr(&self.keys.stats, status_key, 1)
            .query_async(&mut redis)
            .await?;

        Ok(())
    }

    pub async fn retry_loop(&self, cancel_token: CancellationToken) -> Result<(), OxanusError> {
        tracing::info!("Starting retry loop");

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    return Ok(());
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(300)) => {
                    self.track_redis_result(self.enqueue_scheduled(&self.keys.retry).await)?;
                }
            }
        }
    }

    pub async fn schedule_loop(&self, cancel_token: CancellationToken) -> Result<(), OxanusError> {
        tracing::info!("Starting schedule loop");

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    return Ok(());
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(300)) => {
                    self.track_redis_result(self.enqueue_scheduled(&self.keys.schedule).await)?;
                }
            }
        }
    }

    pub async fn cleanup_loop(&self, cancel_token: CancellationToken) -> Result<(), OxanusError> {
        tracing::info!("Starting cleanup loop");

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    return Ok(());
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(600)) => {
                    self.track_redis_result(self.cleanup().await)?;
                }
            }
        }
    }

    pub async fn ping_loop(&self, cancel_token: CancellationToken) -> Result<(), OxanusError> {
        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    return Ok(());
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                    self.track_redis_result(self.ping().await)?;
                }
            }
        }
    }

    pub async fn ping(&self) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        let process = self.current_process();
        let _: () = redis::pipe()
            .zadd(
                &self.keys.processes,
                process.id(),
                chrono::Utc::now().timestamp(),
            )
            .hset(
                &self.keys.processes_data,
                process.id(),
                serde_json::to_string(&process)?,
            )
            .query_async(&mut redis)
            .await?;
        Ok(())
    }

    pub async fn cleanup(&self) -> Result<usize, OxanusError> {
        let mut redis = self.connection().await?;

        let job_ids_to_clean = {
            let now = chrono::Utc::now().timestamp();
            let mut job_ids = vec![];
            let mut cursor = 0;

            loop {
                let mut cmd = redis::cmd("HSCAN");
                cmd.arg(&self.keys.jobs).arg(cursor);
                let result: (u64, Vec<String>) = cmd.query_async(&mut redis).await?;
                let (new_cursor, items) = result;

                let mut iter = items.into_iter();
                while let (Some(job_id), Some(job_str)) = (iter.next(), iter.next()) {
                    let parsed: Result<JobEnvelope, _> = serde_json::from_str(&job_str);

                    match parsed {
                        Ok(job_envelope) => {
                            if job_envelope.meta.created_at_secs() < now - JOB_EXPIRE_TIME {
                                job_ids.push(job_id);
                            }
                        }
                        Err(_) => job_ids.push(job_id),
                    }
                }

                cursor = new_cursor;

                if cursor == 0 {
                    break;
                }
            }

            job_ids
        };

        let count = job_ids_to_clean.len();
        if count > 0 {
            tracing::info!("Cleaning up {count} expired jobs");
            let () = redis.hdel(&self.keys.jobs, job_ids_to_clean).await?;
        }

        Ok(count)
    }

    pub async fn get_process_data(&self, id: &str) -> Result<Option<Process>, OxanusError> {
        let mut redis = self.connection().await?;
        let process_str: Option<String> = (*redis).hget(&self.keys.processes_data, id).await?;
        match process_str {
            Some(process_str) => Ok(Some(serde_json::from_str(&process_str)?)),
            None => Ok(None),
        }
    }

    pub async fn processes(&self) -> Result<Vec<Process>, OxanusError> {
        let mut redis = self.connection().await?;
        let process_ids: Vec<String> = (*redis)
            .zrangebyscore(
                &self.keys.processes,
                chrono::Utc::now().timestamp() - RESURRECT_THRESHOLD_SECS,
                chrono::Utc::now().timestamp(),
            )
            .await?;

        let mut processes = vec![];

        for process_id in process_ids {
            if let Some(process) = self.get_process_data(&process_id).await? {
                processes.push(process);
            }
        }

        Ok(processes)
    }

    pub async fn resurrect_loop(&self, cancel_token: CancellationToken) -> Result<(), OxanusError> {
        tracing::info!("Starting resurrect loop");

        self.ping().await?;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    return Ok(());
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
                    self.track_redis_result(self.resurrect().await)?;
                }
            }
        }
    }

    pub async fn cron_job_loop(
        &self,
        cancel_token: CancellationToken,
        job_name: String,
        cron_job: CronJob,
    ) -> Result<(), OxanusError> {
        let iterator = cron_job
            .schedule
            .after(&(chrono::Utc::now() + chrono::Duration::seconds(3)));

        let mut previous: Option<chrono::DateTime<chrono::Utc>> = None;

        for next in iterator {
            loop {
                if cancel_token.is_cancelled() {
                    return Ok(());
                }

                let now = chrono::Utc::now();
                let max_schedule_time = now + chrono::Duration::minutes(30);

                if next > max_schedule_time {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }

                if let Some(previous) = previous
                    && previous > now
                {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }

                break;
            }

            let scheduled_at = next.timestamp_micros();
            let job_id = format!("{job_name}-{scheduled_at}");

            loop {
                if cancel_token.is_cancelled() {
                    return Ok(());
                }

                let envelope = JobEnvelope::new_cron(
                    cron_job.queue_key.clone(),
                    job_id.clone(),
                    job_name.clone(),
                    scheduled_at,
                    cron_job.resurrect,
                )?;

                match self.track_redis_result(self.enqueue_at(envelope).await)? {
                    Some(_) => break,
                    None => {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }

            previous = Some(next);
        }

        Ok(())
    }

    pub async fn resurrect(&self) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        for process_id in self.dead_process_ids(&mut redis).await? {
            tracing::info!("Dead process detected: {}", process_id);

            let processing_queue = self.processing_queue(&process_id);
            let mut resurrected_count = 0;

            loop {
                let job_ids: Vec<String> = (*redis)
                    .lpop(&processing_queue, Some(NonZero::new(10).unwrap()))
                    .await?;

                if job_ids.is_empty() {
                    break;
                }

                resurrected_count += job_ids.len();

                for job_id in job_ids {
                    match self.get_job_w_conn(&mut redis, &job_id).await? {
                        Some(envelope) => {
                            if envelope.meta.resurrect {
                                tracing::info!(
                                    job_id = job_id,
                                    queue = envelope.queue,
                                    worker = envelope.job.name,
                                    "Resurrecting job"
                                );
                                let _: () = (*redis)
                                    .lpush(self.namespace_queue(&envelope.queue), &envelope.id)
                                    .await?;
                            } else {
                                tracing::info!(
                                    job_id = job_id,
                                    queue = envelope.queue,
                                    worker = envelope.job.name,
                                    "Skipping resurrection (resurrect=false), deleting job"
                                );
                                let _: () = (*redis).hdel(&self.keys.jobs, &envelope.id).await?;
                            }
                        }
                        None => tracing::warn!("Job {} not found", job_id),
                    }
                }
            }

            let _: () = redis::pipe()
                .zrem(&self.keys.processes, &process_id)
                .hdel(&self.keys.processes_data, &process_id)
                .query_async(&mut redis)
                .await?;

            if resurrected_count > 0 {
                tracing::info!(
                    "Resurrected process: {} ({} jobs)",
                    process_id,
                    resurrected_count
                );
            }
        }

        Ok(())
    }

    pub async fn self_cleanup(&self) -> Result<(), OxanusError> {
        let mut redis = self.connection().await?;
        let process = self.current_process();
        let _: () = redis::pipe()
            .zrem(&self.keys.processes, process.id())
            .hdel(&self.keys.processes_data, process.id())
            .query_async(&mut redis)
            .await?;

        Ok(())
    }

    fn processing_queue(&self, process_id: &str) -> String {
        format!("{}:{}", self.keys.processing_queue_prefix, process_id)
    }

    fn current_processing_queue(&self) -> String {
        self.processing_queue(&self.current_process().id())
    }

    #[cfg(test)]
    async fn currently_processing_job_ids(&self) -> Result<Vec<String>, OxanusError> {
        let mut redis = self.connection().await?;
        let job_id: Option<String> = (*redis).lindex(self.current_processing_queue(), 0).await?;
        Ok(job_id.into_iter().collect())
    }

    fn current_process(&self) -> Process {
        let hostname = gethostname::gethostname().to_string_lossy().to_string();
        let pid = std::process::id();
        Process {
            hostname,
            pid,
            heartbeat_at: chrono::Utc::now().timestamp(),
            started_at: self.started_at,
        }
    }

    fn namespace_queue(&self, queue: &str) -> String {
        if queue.starts_with(self.keys.namespace.as_str()) {
            queue.to_string()
        } else {
            format!("{}:{}", self.keys.queue_prefix, queue)
        }
    }

    async fn dead_process_ids(
        &self,
        redis: &mut deadpool_redis::Connection,
    ) -> Result<Vec<String>, OxanusError> {
        let process_ids: Vec<(String, f64)> = (*redis)
            .zrange_withscores(&self.keys.processes, 0, -1)
            .await?;

        let all_process_ids: Vec<String> = process_ids.iter().map(|(id, _)| id.clone()).collect();
        let mut dead_process_ids: Vec<String> = process_ids
            .iter()
            .filter(|(_, score)| {
                *score < (chrono::Utc::now().timestamp() - RESURRECT_THRESHOLD_SECS) as f64
            })
            .map(|(id, _)| id.clone())
            .collect();

        let all_processing_queues: Vec<String> = redis
            .keys(format!("{}:*", self.keys.processing_queue_prefix))
            .await?;

        for processing_queue in all_processing_queues {
            let process_id = match processing_queue.rsplit(':').next() {
                Some(process_id) => process_id.to_string(),
                None => continue,
            };

            if !all_process_ids.contains(&process_id) {
                dead_process_ids.push(process_id);
            }
        }

        Ok(dead_process_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as oxanus;
    use crate::Queue;
    use crate::test_helper::{random_string, redis_pool};
    use rand::RngExt;
    use serde::Serialize;
    use testresult::TestResult;

    #[derive(oxanus::Registry)]
    #[allow(dead_code)]
    struct ComponentRegistry(oxanus::ComponentRegistry<(), ()>);

    #[derive(Serialize, oxanus::Queue)]
    #[oxanus(key = "test")]
    struct TestQueue;

    #[derive(Serialize)]
    struct TestJob {}

    impl crate::worker::Job for TestJob {
        fn worker_name() -> &'static str {
            "TestJob"
        }
    }

    #[tokio::test]
    async fn test_ping() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        storage.ping().await?;

        let process = storage.current_process();
        let process_data = storage.get_process_data(&process.id()).await?;
        assert!(process_data.is_some());
        let process = process_data.unwrap();
        assert_eq!(
            process.hostname,
            gethostname::gethostname().to_string_lossy().to_string()
        );
        assert_eq!(process.pid, std::process::id());
        assert!(process.heartbeat_at > chrono::Utc::now().timestamp() - 3);

        Ok(())
    }

    #[tokio::test]
    async fn test_latency() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();

        let mut envelope = JobEnvelope::new(queue.clone(), TestJob {})?;
        let now = chrono::Utc::now();
        let actual_latency = 7777;
        let past = now.timestamp_micros() - actual_latency * 1_000;
        envelope.meta.created_at = past;
        envelope.meta.scheduled_at = past;
        storage.enqueue(envelope).await?;

        let latency = storage.latency_ms(&queue).await?;

        assert!((latency - actual_latency as f64).abs() < 50.0);

        Ok(())
    }

    #[tokio::test]
    async fn test_latency_multiple_jobs() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();

        let latency_ms = storage.latency_ms(&queue).await?;
        assert_eq!(latency_ms, 0.0);

        let latency_micros = storage.latency_micros(&queue).await?;
        assert_eq!(latency_micros, 0.0);

        let mut envelope = JobEnvelope::new(queue.clone(), TestJob {})?;
        let now = chrono::Utc::now();
        let actual_latency_ms = 7777;
        let actual_latency_micros = actual_latency_ms * 1_000;
        let actual_latency_s = actual_latency_ms as f64 / 1_000.0;
        let past = now.timestamp_micros() - actual_latency_micros;
        envelope.meta.created_at = past;
        envelope.meta.scheduled_at = past;
        storage.enqueue(envelope).await?;

        let latency_ms = storage.latency_ms(&queue).await?;
        assert!((latency_ms - actual_latency_ms as f64).abs() < 50.0);

        let latency_micros = storage.latency_micros(&queue).await?;
        assert!((latency_micros - actual_latency_micros as f64).abs() < 50_000.0);

        let latency_s = latency_micros / 1_000_000.0;
        assert!((latency_s - actual_latency_s).abs() < 0.05);

        let mut envelope2 = JobEnvelope::new(queue.clone(), TestJob {})?;
        let actual_latency_ms2 = 5000;
        let actual_latency_micros2 = actual_latency_ms2 * 1_000;
        let past2 = now.timestamp_micros() - actual_latency_micros2;
        envelope2.meta.created_at = past2;
        envelope2.meta.scheduled_at = past2;
        storage.enqueue(envelope2).await?;

        let latency_ms = storage.latency_ms(&queue).await?;
        assert!((latency_ms - actual_latency_ms as f64).abs() < 50.0);

        let latency_micros = storage.latency_micros(&queue).await?;
        assert!((latency_micros - actual_latency_micros as f64).abs() < 50_000.0);

        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();
        let mut expired_envelope1 = JobEnvelope::new(queue.clone(), TestJob {})?;
        expired_envelope1.meta.created_at =
            (chrono::Utc::now().timestamp() - JOB_EXPIRE_TIME - 1) * 1000000;
        let mut expired_envelope2 = JobEnvelope::new(queue.clone(), TestJob {})?;
        expired_envelope2.meta.created_at =
            (chrono::Utc::now().timestamp() - JOB_EXPIRE_TIME - 1) * 1000000;

        let active_envelope = JobEnvelope::new(queue.clone(), TestJob {})?;

        storage.enqueue(expired_envelope1.clone()).await?;
        storage.enqueue(expired_envelope2.clone()).await?;
        storage.enqueue(active_envelope.clone()).await?;

        assert!(storage.get_job(&expired_envelope1.id).await?.is_some());
        assert!(storage.get_job(&expired_envelope2.id).await?.is_some());
        assert!(storage.get_job(&active_envelope.id).await?.is_some());

        assert_eq!(storage.cleanup().await?, 2);

        assert!(storage.get_job(&expired_envelope1.id).await?.is_none());
        assert!(storage.get_job(&expired_envelope2.id).await?.is_none());
        assert!(storage.get_job(&active_envelope.id).await?.is_some());

        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup_empty() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));

        assert_eq!(storage.cleanup().await?, 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_resurrect() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();
        let envelope = JobEnvelope::new(queue.clone(), TestJob {})?;

        storage.enqueue(envelope.clone()).await?;

        assert_eq!(storage.enqueued_count(&queue).await?, 1);
        assert!(storage.currently_processing_job_ids().await?.is_empty());

        let job_id = storage.dequeue(&queue).await?;

        assert_eq!(job_id, Some(envelope.id));

        assert_eq!(storage.enqueued_count(&queue).await?, 0);
        assert_eq!(
            storage.currently_processing_job_ids().await?,
            vec![job_id.unwrap()]
        );

        let mut redis = storage.connection().await?;

        // fake ping in the past
        let _: () = redis
            .zadd(
                &storage.keys.processes,
                storage.current_process().id(),
                chrono::Utc::now().timestamp() - RESURRECT_THRESHOLD_SECS - 1,
            )
            .await?;

        storage.resurrect().await?;

        assert_eq!(storage.enqueued_count(&queue).await?, 1);
        assert!(storage.currently_processing_job_ids().await?.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_resurrect_unique_job() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();
        let envelope = JobEnvelope::new_cron(
            queue.clone(),
            "CronWorker-1234567890".to_string(),
            "CronWorker".to_string(),
            1234567890,
            true,
        )?;

        assert!(envelope.meta.unique);
        assert_eq!(envelope.meta.on_conflict, Some(JobConflictStrategy::Skip));

        storage.enqueue(envelope.clone()).await?;

        assert_eq!(storage.enqueued_count(&queue).await?, 1);
        assert!(storage.currently_processing_job_ids().await?.is_empty());
        assert!(storage.get_job(&envelope.id).await?.is_some());

        let job_id = storage.dequeue(&queue).await?;

        assert_eq!(job_id, Some(envelope.id.clone()));

        assert_eq!(storage.enqueued_count(&queue).await?, 0);
        assert_eq!(
            storage.currently_processing_job_ids().await?,
            vec![job_id.expect("job_id should be Some")]
        );

        let mut redis = storage.connection().await?;

        let _: () = redis
            .zadd(
                &storage.keys.processes,
                storage.current_process().id(),
                chrono::Utc::now().timestamp() - RESURRECT_THRESHOLD_SECS - 1,
            )
            .await?;

        storage.resurrect().await?;

        assert_eq!(storage.enqueued_count(&queue).await?, 1);
        assert!(storage.currently_processing_job_ids().await?.is_empty());
        assert!(
            storage.get_job(&envelope.id).await?.is_some(),
            "unique job should still exist in jobs hash after resurrection"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_resurrect_when_process_is_missing() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();
        let envelope = JobEnvelope::new(queue.clone(), TestJob {})?;

        storage.enqueue(envelope.clone()).await?;

        assert_eq!(storage.enqueued_count(&queue).await?, 1);
        assert!(storage.currently_processing_job_ids().await?.is_empty());

        let job_id = storage.dequeue(&queue).await?;

        assert_eq!(job_id, Some(envelope.id));

        assert_eq!(storage.enqueued_count(&queue).await?, 0);
        assert_eq!(
            storage.currently_processing_job_ids().await?,
            vec![job_id.unwrap()]
        );

        storage.resurrect().await?;

        assert_eq!(storage.enqueued_count(&queue).await?, 1);
        assert!(storage.currently_processing_job_ids().await?.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_get_many_with_missing_jobs() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();

        let envelope1 = JobEnvelope::new(queue.clone(), TestJob {})?;
        let envelope2 = JobEnvelope::new(queue.clone(), TestJob {})?;
        let envelope3 = JobEnvelope::new(queue.clone(), TestJob {})?;

        storage.enqueue(envelope1.clone()).await?;
        storage.enqueue(envelope2.clone()).await?;
        storage.enqueue(envelope3.clone()).await?;

        storage.delete_job(&envelope2.id).await?;

        let job_ids = vec![
            envelope1.id.clone(),
            envelope2.id.clone(),
            envelope3.id.clone(),
        ];
        let envelopes = storage.get_many(&job_ids).await?;

        assert_eq!(envelopes.len(), 2);

        let returned_ids: Vec<String> = envelopes.iter().map(|e| e.id.clone()).collect();
        assert!(returned_ids.contains(&envelope1.id));
        assert!(!returned_ids.contains(&envelope2.id));
        assert!(returned_ids.contains(&envelope3.id));

        Ok(())
    }

    #[tokio::test]
    async fn test_list_queue_jobs() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();

        let envelope1 = JobEnvelope::new(queue.clone(), TestJob {})?;
        let envelope2 = JobEnvelope::new(queue.clone(), TestJob {})?;
        let envelope3 = JobEnvelope::new(queue.clone(), TestJob {})?;

        storage.enqueue(envelope1.clone()).await?;
        storage.enqueue(envelope2.clone()).await?;
        storage.enqueue(envelope3.clone()).await?;

        let opts = QueueListOpts {
            count: 10,
            offset: 0,
        };
        let jobs = storage.list_queue_jobs(&queue, &opts).await?;
        assert_eq!(jobs.len(), 3);

        let opts = QueueListOpts {
            count: 2,
            offset: 0,
        };
        let jobs = storage.list_queue_jobs(&queue, &opts).await?;
        assert_eq!(jobs.len(), 2);

        let opts = QueueListOpts {
            count: 10,
            offset: 1,
        };
        let jobs = storage.list_queue_jobs(&queue, &opts).await?;
        assert_eq!(jobs.len(), 2);

        let opts = QueueListOpts {
            count: 10,
            offset: 5,
        };
        let jobs = storage.list_queue_jobs(&queue, &opts).await?;
        assert!(jobs.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_wipe_queue() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();

        let envelope1 = JobEnvelope::new(queue.clone(), TestJob {})?;
        let envelope2 = JobEnvelope::new(queue.clone(), TestJob {})?;

        storage.enqueue(envelope1.clone()).await?;
        storage.enqueue(envelope2.clone()).await?;

        assert_eq!(storage.enqueued_count(&queue).await?, 2);
        assert!(storage.get_job(&envelope1.id).await?.is_some());
        assert!(storage.get_job(&envelope2.id).await?.is_some());

        storage.wipe_queue(&queue).await?;

        assert_eq!(storage.enqueued_count(&queue).await?, 0);
        assert!(storage.get_job(&envelope1.id).await?.is_none());
        assert!(storage.get_job(&envelope2.id).await?.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_set_started_at() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();

        let mut envelope = JobEnvelope::new(queue.clone(), TestJob {})?;
        assert!(envelope.meta.started_at.is_none());

        storage.enqueue(envelope.clone()).await?;

        let before = chrono::Utc::now().timestamp_micros();
        storage.set_started_at(&mut envelope).await?;
        let after = chrono::Utc::now().timestamp_micros();

        let started_at = envelope.meta.started_at.expect("started_at should be set");
        assert!(started_at >= before);
        assert!(started_at <= after);

        let persisted = storage
            .get_job(&envelope.id)
            .await?
            .expect("job should exist");
        assert_eq!(persisted.meta.started_at, Some(started_at));

        Ok(())
    }

    #[tokio::test]
    async fn test_enqueue_envelope() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();

        let now = chrono::Utc::now().timestamp_micros();
        let id = uuid::Uuid::new_v4().to_string();
        let envelope = JobEnvelope {
            id: id.clone(),
            queue: queue.clone(),
            job: crate::job_envelope::JobData {
                name: "MyWorker".to_string(),
                args: serde_json::json!({"key": "value"}),
            },
            meta: crate::job_envelope::JobMeta {
                id: id.clone(),
                retries: 0,
                unique: false,
                on_conflict: None,
                created_at: now,
                scheduled_at: now,
                started_at: None,
                state: None,
                resurrect: true,
                error: None,
                throttle_cost: None,
            },
        };

        let returned_id = storage.enqueue(envelope).await?;
        assert_eq!(returned_id, id);
        assert_eq!(storage.enqueued_count(&queue).await?, 1);

        let job = storage.get_job(&id).await?.expect("job should exist");
        assert_eq!(job.queue, queue);
        assert_eq!(job.job.name, "MyWorker");
        assert_eq!(job.job.args, serde_json::json!({"key": "value"}));
        assert_eq!(job.meta.retries, 0);
        assert!(job.meta.error.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_enqueue_in_envelope() -> TestResult {
        let storage = StorageInternal::new(redis_pool().await?, Some(random_string()));
        let queue = random_string();

        let now = chrono::Utc::now().timestamp_micros();
        let id = uuid::Uuid::new_v4().to_string();
        let envelope = JobEnvelope {
            id: id.clone(),
            queue: queue.clone(),
            job: crate::job_envelope::JobData {
                name: "MyWorker".to_string(),
                args: serde_json::json!({"key": "value"}),
            },
            meta: crate::job_envelope::JobMeta {
                id: id.clone(),
                retries: 0,
                unique: false,
                on_conflict: None,
                created_at: now,
                scheduled_at: now,
                started_at: None,
                state: None,
                resurrect: true,
                error: None,
                throttle_cost: None,
            },
        };
        let delay_s = rand::rng().random_range(1..3600);
        let delay = chrono::Duration::seconds(delay_s)
            .num_microseconds()
            .unwrap();

        let before = chrono::Utc::now().timestamp_micros();
        let returned_id = storage.enqueue_in(envelope, delay_s as u64).await?;
        let after = chrono::Utc::now().timestamp_micros();
        assert_eq!(returned_id, id);
        assert_eq!(storage.enqueued_count(&queue).await?, 0);
        assert_eq!(storage.scheduled_count().await?, 1);

        let job = storage.get_job(&id).await?.expect("job should exist");
        assert_eq!(job.queue, queue);
        assert_eq!(job.job.name, "MyWorker");
        assert_eq!(job.job.args, serde_json::json!({"key": "value"}));

        assert!(job.meta.scheduled_at >= (before + delay));
        assert!(job.meta.scheduled_at <= (after + delay));
        assert_eq!(job.meta.retries, 0);
        assert!(job.meta.error.is_none());

        Ok(())
    }
    #[tokio::test]
    async fn test_envelope_new_scheduled() -> TestResult {
        let queue = random_string();
        let delay_s = rand::rng().random_range(1..3600);
        let scheduled_at = chrono::Utc::now() + chrono::Duration::seconds(delay_s);

        let envelope = JobEnvelope::new_scheduled(queue.clone(), TestJob {}, scheduled_at)?;

        assert_eq!(envelope.meta.scheduled_at, scheduled_at.timestamp_micros());

        Ok(())
    }

    #[tokio::test]
    async fn test_enqueue_at_job_envelope() -> TestResult {
        let storage = crate::Storage::builder().build_from_env()?;
        let internal_storage = &storage.internal;

        let delay = rand::rng().random_range(1_000_000..10_000_000);
        let scheduled_at = chrono::Utc::now() + chrono::Duration::microseconds(delay);

        let before = chrono::Utc::now().timestamp_micros();
        let returned_id = storage
            .enqueue_at(TestQueue, TestJob {}, scheduled_at)
            .await?;
        let after = chrono::Utc::now().timestamp_micros();
        assert_eq!(
            internal_storage.enqueued_count(&TestQueue {}.key()).await?,
            0
        );
        assert_eq!(internal_storage.scheduled_count().await?, 1);

        let job = internal_storage
            .get_job(&returned_id)
            .await?
            .expect("job should exist");
        assert_eq!(job.queue, TestQueue {}.key());

        assert!(job.meta.scheduled_at >= before + delay);
        assert!(job.meta.scheduled_at <= after + delay);
        assert_eq!(job.meta.retries, 0);
        assert!(job.meta.error.is_none());

        Ok(())
    }
}
