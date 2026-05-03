//! Sidekiq-style execution metrics for Oxanus workers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::result_collector::{WorkerResult, WorkerResultKind};

pub(crate) const METRICS_RETENTION_SECS: i64 = 8 * 60 * 60;
pub(crate) const DEFAULT_METRIC_MINUTES: usize = 60;
pub(crate) const MAX_METRIC_MINUTES: usize = 8 * 60;
pub(crate) const HISTOGRAM_BUCKET_COUNT: usize = 14;

/// Maximum execution time represented by each histogram bucket.
pub const HISTOGRAM_BUCKET_INTERVALS_MS: [u64; HISTOGRAM_BUCKET_COUNT] = [
    25,
    50,
    100,
    250,
    500,
    1000,
    2500,
    5000,
    10000,
    30000,
    60000,
    120000,
    300000,
    u64::MAX,
];

/// Display labels for [`HISTOGRAM_BUCKET_INTERVALS_MS`].
pub const HISTOGRAM_BUCKET_LABELS: [&str; HISTOGRAM_BUCKET_COUNT] = [
    "25ms", "50ms", "100ms", "250ms", "500ms", "1s", "2.5s", "5s", "10s", "30s", "60s", "120s",
    "5min", "Slow",
];

pub(crate) const METRIC_PROCESSED_JOBS: &str = "p";
pub(crate) const METRIC_FAILED_JOBS: &str = "f";
pub(crate) const METRIC_PANICKED_JOBS: &str = "pn";
pub(crate) const METRIC_SUCCESSFUL_EXECUTIONS: &str = "xs";
pub(crate) const METRIC_FAILED_EXECUTIONS: &str = "xf";
pub(crate) const METRIC_PANICKED_EXECUTIONS: &str = "xpn";
pub(crate) const METRIC_EXECUTION_MS: &str = "ms";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MetricIdentity {
    pub worker: String,
}

impl MetricIdentity {
    pub(crate) fn from_worker_result(result: &WorkerResult) -> Self {
        Self {
            worker: result.worker_name.clone(),
        }
    }

    pub(crate) fn field_key(&self) -> String {
        format!("{}:{}", self.worker.len(), self.worker)
    }

    pub(crate) fn from_field_key(key: &str) -> Option<Self> {
        let (worker_len, rest) = key.split_once(':')?;
        let worker_len = worker_len.parse::<usize>().ok()?;

        if rest.len() < worker_len || !rest.is_char_boundary(worker_len) {
            return None;
        }

        let (worker, _) = rest.split_at(worker_len);
        Some(Self {
            worker: worker.to_string(),
        })
    }

    pub(crate) fn metric_field(&self, metric: &str) -> String {
        format!("{}|{metric}", self.field_key())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct JobMetricsQuery {
    pub minutes: usize,
}

impl JobMetricsQuery {
    #[must_use]
    pub fn new(minutes: usize) -> Self {
        Self { minutes }
    }

    #[must_use]
    pub fn effective_minutes(&self) -> usize {
        if self.minutes == 0 {
            DEFAULT_METRIC_MINUTES
        } else {
            self.minutes.min(MAX_METRIC_MINUTES)
        }
    }
}

impl Default for JobMetricsQuery {
    fn default() -> Self {
        Self {
            minutes: DEFAULT_METRIC_MINUTES,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobMetricsTotals {
    /// Jobs completed, including successes, failures, and panics. Batch jobs count individually.
    pub processed: u64,
    /// Jobs completed successfully. Batch jobs count individually.
    pub succeeded: u64,
    /// Jobs that failed or panicked. Batch jobs count individually.
    pub failed: u64,
    /// Jobs that panicked. Batch jobs count individually.
    pub panicked: u64,
    /// Successful worker executions. Batch workers count once per batch.
    pub successful_executions: u64,
    /// Worker executions that failed or panicked. Batch workers count once per batch.
    pub failed_executions: u64,
    /// Panicked worker executions. Batch workers count once per batch.
    pub panicked_executions: u64,
    /// Total duration for successful worker executions. Batch duration counts once per batch.
    pub execution_ms: u64,
}

impl JobMetricsTotals {
    #[must_use]
    pub fn average_execution_ms(&self) -> f64 {
        if self.successful_executions == 0 {
            0.0
        } else {
            self.execution_ms as f64 / self.successful_executions as f64
        }
    }

    #[must_use]
    pub fn execution_seconds(&self) -> f64 {
        self.execution_ms as f64 / 1000.0
    }

    #[must_use]
    pub fn failed_executions_without_panics(&self) -> u64 {
        self.failed_executions
            .saturating_sub(self.panicked_executions)
    }

    fn add_metric(&mut self, metric: &str, value: u64) {
        match metric {
            METRIC_PROCESSED_JOBS => self.processed = self.processed.saturating_add(value),
            METRIC_FAILED_JOBS => self.failed = self.failed.saturating_add(value),
            METRIC_PANICKED_JOBS => self.panicked = self.panicked.saturating_add(value),
            METRIC_SUCCESSFUL_EXECUTIONS => {
                self.successful_executions = self.successful_executions.saturating_add(value);
            }
            METRIC_FAILED_EXECUTIONS => {
                self.failed_executions = self.failed_executions.saturating_add(value);
            }
            METRIC_PANICKED_EXECUTIONS => {
                self.panicked_executions = self.panicked_executions.saturating_add(value);
            }
            METRIC_EXECUTION_MS => self.execution_ms = self.execution_ms.saturating_add(value),
            _ => {}
        }
    }

    fn finalize(&mut self) {
        self.succeeded = self.processed.saturating_sub(self.failed);
        if self.successful_executions + self.failed_executions + self.panicked_executions == 0 {
            self.successful_executions = self.succeeded;
            self.failed_executions = self.failed;
            self.panicked_executions = self.panicked;
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobMetricsPoint {
    /// Start timestamp for this minute bucket.
    pub timestamp: i64,
    /// Jobs completed, including successes, failures, and panics. Batch jobs count individually.
    pub processed: u64,
    /// Jobs completed successfully. Batch jobs count individually.
    pub succeeded: u64,
    /// Jobs that failed or panicked. Batch jobs count individually.
    pub failed: u64,
    /// Jobs that panicked. Batch jobs count individually.
    pub panicked: u64,
    /// Successful worker executions. Batch workers count once per batch.
    pub successful_executions: u64,
    /// Worker executions that failed or panicked. Batch workers count once per batch.
    pub failed_executions: u64,
    /// Panicked worker executions. Batch workers count once per batch.
    pub panicked_executions: u64,
    /// Total duration for successful worker executions. Batch duration counts once per batch.
    pub execution_ms: u64,
}

impl JobMetricsPoint {
    #[must_use]
    pub fn average_execution_ms(&self) -> f64 {
        if self.successful_executions == 0 {
            0.0
        } else {
            self.execution_ms as f64 / self.successful_executions as f64
        }
    }

    #[must_use]
    pub fn failed_executions_without_panics(&self) -> u64 {
        self.failed_executions
            .saturating_sub(self.panicked_executions)
    }

    fn add_metric(&mut self, metric: &str, value: u64) {
        match metric {
            METRIC_PROCESSED_JOBS => self.processed = self.processed.saturating_add(value),
            METRIC_FAILED_JOBS => self.failed = self.failed.saturating_add(value),
            METRIC_PANICKED_JOBS => self.panicked = self.panicked.saturating_add(value),
            METRIC_SUCCESSFUL_EXECUTIONS => {
                self.successful_executions = self.successful_executions.saturating_add(value);
            }
            METRIC_FAILED_EXECUTIONS => {
                self.failed_executions = self.failed_executions.saturating_add(value);
            }
            METRIC_PANICKED_EXECUTIONS => {
                self.panicked_executions = self.panicked_executions.saturating_add(value);
            }
            METRIC_EXECUTION_MS => self.execution_ms = self.execution_ms.saturating_add(value),
            _ => {}
        }
    }

    fn finalize(&mut self) {
        self.succeeded = self.processed.saturating_sub(self.failed);
        if self.successful_executions + self.failed_executions + self.panicked_executions == 0 {
            self.successful_executions = self.succeeded;
            self.failed_executions = self.failed;
            self.panicked_executions = self.panicked;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetricsSummary {
    /// Worker these metrics belong to.
    pub identity: MetricIdentity,
    /// Totals for this worker over the queried window.
    pub totals: JobMetricsTotals,
    /// Per-minute points for this worker over the queried window.
    pub series: Vec<JobMetricsPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetricsSnapshot {
    /// Inclusive start timestamp for the queried window.
    pub starts_at: i64,
    /// Inclusive end timestamp for the queried window.
    pub ends_at: i64,
    /// Number of minute buckets returned.
    pub minutes: usize,
    /// Totals across all workers in the queried window.
    pub totals: JobMetricsTotals,
    /// Per-minute totals across all workers in the queried window.
    pub series: Vec<JobMetricsPoint>,
    /// Per-worker summaries sorted by total execution time descending.
    pub workers: Vec<WorkerMetricsSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetricsHistogramBucket {
    pub label: String,
    pub upper_bound_ms: Option<u64>,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetricsDetail {
    /// Worker these metrics belong to.
    pub identity: MetricIdentity,
    /// Inclusive start timestamp for the queried window.
    pub starts_at: i64,
    /// Inclusive end timestamp for the queried window.
    pub ends_at: i64,
    /// Number of minute buckets returned.
    pub minutes: usize,
    /// Totals for this worker in the queried window.
    pub totals: JobMetricsTotals,
    /// Per-minute points for this worker in the queried window.
    pub series: Vec<JobMetricsPoint>,
    /// Execution-time histogram for successful worker executions.
    pub histogram: Vec<JobMetricsHistogramBucket>,
}

#[derive(Clone, Default)]
pub(crate) struct JobMetricsBuffer {
    entries: HashMap<(i64, MetricIdentity), PendingJobMetrics>,
}

impl JobMetricsBuffer {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn record(&mut self, result: &WorkerResult) {
        let minute = chrono::Utc::now().timestamp().div_euclid(60);
        let identity = MetricIdentity::from_worker_result(result);
        let metrics = self.entries.entry((minute, identity)).or_default();

        metrics.processed = metrics.processed.saturating_add(result.job_count);

        match result.kind {
            WorkerResultKind::Success => {
                metrics.successful_executions = metrics.successful_executions.saturating_add(1);
                metrics.execution_ms = metrics.execution_ms.saturating_add(result.execution_ms);
                let bucket = histogram_bucket_index(result.execution_ms);
                if let Some(count) = metrics.histogram.get_mut(bucket) {
                    *count = count.saturating_add(1);
                }
            }
            WorkerResultKind::Panicked => {
                metrics.failed = metrics.failed.saturating_add(result.job_count);
                metrics.panicked = metrics.panicked.saturating_add(result.job_count);
                metrics.failed_executions = metrics.failed_executions.saturating_add(1);
                metrics.panicked_executions = metrics.panicked_executions.saturating_add(1);
            }
            WorkerResultKind::Failed => {
                metrics.failed = metrics.failed.saturating_add(result.job_count);
                metrics.failed_executions = metrics.failed_executions.saturating_add(1);
            }
        }
    }

    pub(crate) fn records(
        &self,
    ) -> impl Iterator<Item = (i64, &MetricIdentity, &PendingJobMetrics)> {
        self.entries
            .iter()
            .map(|((minute, identity), metrics)| (*minute, identity, metrics))
    }
}

#[derive(Clone, Default)]
pub(crate) struct PendingJobMetrics {
    pub(crate) processed: u64,
    pub(crate) failed: u64,
    pub(crate) panicked: u64,
    pub(crate) successful_executions: u64,
    pub(crate) failed_executions: u64,
    pub(crate) panicked_executions: u64,
    pub(crate) execution_ms: u64,
    pub(crate) histogram: [u64; HISTOGRAM_BUCKET_COUNT],
}

#[derive(Default)]
pub(crate) struct JobMetricsAggregation {
    pub(crate) totals: JobMetricsTotals,
    pub(crate) series: Vec<JobMetricsPoint>,
    pub(crate) workers: Vec<WorkerMetricsSummary>,
}

struct WorkerMetricsSummaryBuilder {
    totals: JobMetricsTotals,
    series: Vec<JobMetricsPoint>,
}

#[must_use]
pub(crate) fn metric_minutes(now_ts: i64, query: JobMetricsQuery) -> Vec<i64> {
    let minutes = query.effective_minutes();
    let end_minute = now_ts.div_euclid(60);
    let start_minute = end_minute - i64::try_from(minutes).unwrap_or(i64::MAX) + 1;
    (start_minute..=end_minute).collect()
}

#[must_use]
pub(crate) fn aggregate_counter_hashes(
    minutes: &[i64],
    hashes: Vec<HashMap<String, i64>>,
    filter: Option<&MetricIdentity>,
) -> JobMetricsAggregation {
    let mut aggregation = JobMetricsAggregation {
        series: minutes
            .iter()
            .map(|minute| JobMetricsPoint {
                timestamp: minute * 60,
                ..JobMetricsPoint::default()
            })
            .collect(),
        ..JobMetricsAggregation::default()
    };
    let worker_series_template: Vec<JobMetricsPoint> = minutes
        .iter()
        .map(|minute| JobMetricsPoint {
            timestamp: minute * 60,
            ..JobMetricsPoint::default()
        })
        .collect();
    let mut workers: HashMap<MetricIdentity, WorkerMetricsSummaryBuilder> = HashMap::new();

    for (idx, hash) in hashes.into_iter().enumerate() {
        let Some(point) = aggregation.series.get_mut(idx) else {
            continue;
        };

        for (field, raw_value) in hash {
            let Some((identity, metric)) = split_metric_field(&field) else {
                continue;
            };
            if filter.is_some_and(|expected| expected != &identity) {
                continue;
            }

            let value = u64::try_from(raw_value).unwrap_or_default();
            point.add_metric(metric, value);
            aggregation.totals.add_metric(metric, value);
            let worker_summary =
                workers
                    .entry(identity)
                    .or_insert_with(|| WorkerMetricsSummaryBuilder {
                        totals: JobMetricsTotals::default(),
                        series: worker_series_template.clone(),
                    });
            worker_summary.totals.add_metric(metric, value);
            if let Some(worker_point) = worker_summary.series.get_mut(idx) {
                worker_point.add_metric(metric, value);
            }
        }

        point.finalize();
    }

    aggregation.totals.finalize();
    aggregation.workers = workers
        .into_iter()
        .map(|(identity, mut summary)| {
            summary.totals.finalize();
            for point in &mut summary.series {
                point.finalize();
            }
            WorkerMetricsSummary {
                identity,
                totals: summary.totals,
                series: summary.series,
            }
        })
        .collect();
    aggregation.workers.sort_by(|a, b| {
        b.totals
            .execution_ms
            .cmp(&a.totals.execution_ms)
            .then_with(|| b.totals.processed.cmp(&a.totals.processed))
            .then_with(|| a.identity.worker.cmp(&b.identity.worker))
    });

    aggregation
}

#[must_use]
pub(crate) fn histogram_bucket_index(duration_ms: u64) -> usize {
    HISTOGRAM_BUCKET_INTERVALS_MS
        .iter()
        .position(|upper| duration_ms < *upper)
        .unwrap_or(HISTOGRAM_BUCKET_COUNT - 1)
}

#[must_use]
pub(crate) fn histogram_bitfield_increment_args(
    buckets: &[u64; HISTOGRAM_BUCKET_COUNT],
) -> Vec<String> {
    let mut args = vec!["OVERFLOW".to_string(), "SAT".to_string()];
    for (idx, value) in buckets.iter().enumerate() {
        if *value == 0 {
            continue;
        }
        args.push("INCRBY".to_string());
        args.push("u16".to_string());
        args.push(format!("#{idx}"));
        args.push(value.to_string());
    }
    args
}

#[must_use]
pub(crate) fn histogram_bitfield_fetch_args() -> Vec<String> {
    let mut args = Vec::with_capacity(HISTOGRAM_BUCKET_COUNT * 3);
    for idx in 0..HISTOGRAM_BUCKET_COUNT {
        args.push("GET".to_string());
        args.push("u16".to_string());
        args.push(format!("#{idx}"));
    }
    args
}

#[must_use]
pub(crate) fn histogram_buckets_from_counts(
    counts: &[u64; HISTOGRAM_BUCKET_COUNT],
) -> Vec<JobMetricsHistogramBucket> {
    HISTOGRAM_BUCKET_LABELS
        .iter()
        .zip(counts.iter())
        .enumerate()
        .map(|(idx, (label, count))| JobMetricsHistogramBucket {
            label: (*label).to_string(),
            upper_bound_ms: (idx < HISTOGRAM_BUCKET_COUNT - 1)
                .then(|| HISTOGRAM_BUCKET_INTERVALS_MS.get(idx).copied())
                .flatten(),
            count: *count,
        })
        .collect()
}

fn split_metric_field(field: &str) -> Option<(MetricIdentity, &str)> {
    let (identity_key, metric) = field.rsplit_once('|')?;
    match metric {
        METRIC_PROCESSED_JOBS
        | METRIC_FAILED_JOBS
        | METRIC_PANICKED_JOBS
        | METRIC_SUCCESSFUL_EXECUTIONS
        | METRIC_FAILED_EXECUTIONS
        | METRIC_PANICKED_EXECUTIONS
        | METRIC_EXECUTION_MS => {
            MetricIdentity::from_field_key(identity_key).map(|id| (id, metric))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn histogram_bucket_boundaries_use_configured_thresholds() {
        assert_eq!(histogram_bucket_index(0), 0);
        assert_eq!(histogram_bucket_index(24), 0);
        assert_eq!(histogram_bucket_index(25), 1);
        assert_eq!(histogram_bucket_index(49), 1);
        assert_eq!(histogram_bucket_index(50), 2);
        assert_eq!(histogram_bucket_index(299_999), 12);
        assert_eq!(histogram_bucket_index(300_000), 13);
    }

    #[test]
    fn bitfield_increment_args_use_saturated_u16_counters() {
        let mut buckets = [0_u64; HISTOGRAM_BUCKET_COUNT];
        buckets[0] = 2;
        buckets[13] = 70_000;

        let args = histogram_bitfield_increment_args(&buckets);

        assert_eq!(
            args,
            vec![
                "OVERFLOW", "SAT", "INCRBY", "u16", "#0", "2", "INCRBY", "u16", "#13", "70000",
            ]
        );
    }

    #[test]
    fn query_aggregation_computes_totals_and_clamps_minutes() {
        let identity = MetricIdentity {
            worker: "WorkerA".to_string(),
        };
        let other = MetricIdentity {
            worker: "WorkerB".to_string(),
        };
        let minutes = metric_minutes(10_000, JobMetricsQuery::new(999));
        assert_eq!(minutes.len(), MAX_METRIC_MINUTES);

        let mut hashes = vec![HashMap::new(); minutes.len()];
        let first_hash = hashes
            .get_mut(0)
            .expect("query should include a first minute bucket");
        first_hash.insert(identity.metric_field(METRIC_PROCESSED_JOBS), 3);
        first_hash.insert(identity.metric_field(METRIC_FAILED_JOBS), 1);
        first_hash.insert(identity.metric_field(METRIC_SUCCESSFUL_EXECUTIONS), 2);
        first_hash.insert(identity.metric_field(METRIC_FAILED_EXECUTIONS), 1);
        first_hash.insert(identity.metric_field(METRIC_EXECUTION_MS), 250);

        let second_hash = hashes
            .get_mut(1)
            .expect("query should include a second minute bucket");
        second_hash.insert(other.metric_field(METRIC_PROCESSED_JOBS), 2);
        second_hash.insert(other.metric_field(METRIC_FAILED_JOBS), 1);
        second_hash.insert(other.metric_field(METRIC_PANICKED_JOBS), 1);
        second_hash.insert(other.metric_field(METRIC_SUCCESSFUL_EXECUTIONS), 1);
        second_hash.insert(other.metric_field(METRIC_FAILED_EXECUTIONS), 1);
        second_hash.insert(other.metric_field(METRIC_PANICKED_EXECUTIONS), 1);
        second_hash.insert(other.metric_field(METRIC_EXECUTION_MS), 100);

        let aggregation = aggregate_counter_hashes(&minutes, hashes.clone(), None);
        assert_eq!(aggregation.totals.processed, 5);
        assert_eq!(aggregation.totals.failed, 2);
        assert_eq!(aggregation.totals.panicked, 1);
        assert_eq!(aggregation.totals.succeeded, 3);
        assert_eq!(aggregation.totals.successful_executions, 3);
        assert_eq!(aggregation.totals.failed_executions, 2);
        assert_eq!(aggregation.totals.panicked_executions, 1);
        assert_eq!(aggregation.totals.failed_executions_without_panics(), 1);
        assert_eq!(aggregation.totals.execution_ms, 350);
        assert_eq!(aggregation.workers.len(), 2);
        let first_worker = aggregation
            .workers
            .first()
            .expect("first worker summary should exist");
        let first_point = first_worker
            .series
            .first()
            .expect("first worker should have a first series point");
        assert_eq!(first_worker.series.len(), MAX_METRIC_MINUTES);
        assert_eq!(first_point.processed, 3);
        assert_eq!(first_point.failed, 1);
        assert_eq!(first_point.succeeded, 2);
        assert_eq!(first_point.successful_executions, 2);
        assert_eq!(first_point.execution_ms, 250);

        let second_worker = aggregation
            .workers
            .get(1)
            .expect("second worker summary should exist");
        assert_eq!(second_worker.totals.panicked, 1);
        assert_eq!(second_worker.totals.failed_executions, 1);
        assert_eq!(second_worker.totals.panicked_executions, 1);
        assert_eq!(second_worker.totals.failed_executions_without_panics(), 0);

        let filtered = aggregate_counter_hashes(&minutes, hashes, Some(&identity));
        assert_eq!(filtered.totals.processed, 3);
        assert_eq!(filtered.totals.failed, 1);
        assert_eq!(filtered.totals.succeeded, 2);
        assert_eq!(filtered.totals.successful_executions, 2);
        assert_eq!(filtered.totals.execution_ms, 250);
        assert_eq!(filtered.workers.len(), 1);
        let filtered_worker = filtered
            .workers
            .first()
            .expect("filtered worker summary should exist");
        let empty_point = filtered_worker
            .series
            .get(1)
            .expect("filtered worker should have a second series point");
        assert_eq!(empty_point.processed, 0);
    }

    #[test]
    fn metric_identity_round_trips_with_delimiters() {
        let identity = MetricIdentity {
            worker: "crate::worker|Name".to_string(),
        };

        assert_eq!(
            MetricIdentity::from_field_key(&identity.field_key()),
            Some(identity)
        );
    }

    #[test]
    fn metric_identity_reads_legacy_worker_queue_keys_as_worker_only() {
        let legacy_key = "6:Workerdefault";

        assert_eq!(
            MetricIdentity::from_field_key(legacy_key),
            Some(MetricIdentity {
                worker: "Worker".to_string(),
            })
        );
    }
}
