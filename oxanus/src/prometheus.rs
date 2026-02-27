//! Prometheus metrics integration for Oxanus.
//!
//! This module provides Prometheus metrics based on the [`Stats`] from the storage.
//!
//! # Example
//!
//! ```rust,ignore
//! use oxanus::Storage;
//!
//! async fn example(storage: &Storage) -> Result<(), oxanus::OxanusError> {
//!     let metrics = storage.metrics().await?;
//!
//!     // Encode metrics to text format
//!     let output = metrics.encode_to_string()?;
//!     println!("{}", output);
//!     Ok(())
//! }
//! ```

use prometheus_client::{
    encoding::{EncodeLabelSet, text::encode},
    metrics::{family::Family, gauge::Gauge},
    registry::Registry,
};
use std::sync::atomic::{AtomicI64, AtomicU64};

use crate::stats::Stats;

/// Label set for queue-level metrics.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct QueueLabels {
    /// The queue key/name.
    pub queue: String,
}

/// Label set for dynamic sub-queue metrics.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct DynamicQueueLabels {
    /// The parent queue key/name.
    pub queue: String,
    /// The dynamic queue suffix.
    pub suffix: String,
}

/// Label set for process-level metrics.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ProcessLabels {
    /// The hostname of the process.
    pub hostname: String,
    /// The process ID.
    pub pid: String,
}

/// Prometheus metrics for Oxanus job queue.
///
/// This struct holds all the Prometheus metrics and the registry.
/// Use [`PrometheusMetrics::from_stats()`] to create an instance from storage stats.
pub struct PrometheusMetrics {
    registry: Registry,
}

impl PrometheusMetrics {
    /// Creates a new [`PrometheusMetrics`] instance from the provided stats.
    #[must_use]
    pub fn from_stats(stats: &Stats) -> Self {
        Self::from_stats_with_prefix(stats, "oxanus")
    }

    /// Creates a new [`PrometheusMetrics`] instance from the provided stats with a custom prefix.
    #[must_use]
    pub fn from_stats_with_prefix(stats: &Stats, prefix: &str) -> Self {
        let mut registry = Registry::with_prefix(prefix);

        // Global metrics
        let jobs_total = Gauge::<i64, AtomicI64>::default();
        jobs_total.set(stats.global.jobs as i64);
        registry.register(
            "jobs_total",
            "Total number of jobs (enqueued + scheduled)",
            jobs_total,
        );

        let enqueued_total = Gauge::<i64, AtomicI64>::default();
        enqueued_total.set(stats.global.enqueued as i64);
        registry.register(
            "enqueued_total",
            "Total number of jobs currently enqueued",
            enqueued_total,
        );

        let processed_total = Gauge::<i64, AtomicI64>::default();
        processed_total.set(stats.global.processed);
        registry.register(
            "processed_total",
            "Total number of jobs processed",
            processed_total,
        );

        let failed_total = Gauge::<i64, AtomicI64>::default();
        failed_total.set(stats.global.failed);
        registry.register("failed_total", "Total number of jobs failed", failed_total);

        let dead_total = Gauge::<i64, AtomicI64>::default();
        dead_total.set(stats.global.dead as i64);
        registry.register("dead_total", "Total number of dead jobs", dead_total);

        let scheduled_total = Gauge::<i64, AtomicI64>::default();
        scheduled_total.set(stats.global.scheduled as i64);
        registry.register(
            "scheduled_total",
            "Total number of scheduled jobs",
            scheduled_total,
        );

        let retries_total = Gauge::<i64, AtomicI64>::default();
        retries_total.set(stats.global.retries as i64);
        registry.register(
            "retries_total",
            "Total number of jobs in retry queue",
            retries_total,
        );

        let latency_max_seconds = Gauge::<f64, AtomicU64>::default();
        latency_max_seconds.set(stats.global.latency_s_max);
        registry.register(
            "latency_max_seconds",
            "Maximum latency across all queues in seconds",
            latency_max_seconds,
        );

        // Queue metrics
        let queue_enqueued = Family::<QueueLabels, Gauge<i64, AtomicI64>>::default();
        let queue_processed = Family::<QueueLabels, Gauge<i64, AtomicI64>>::default();
        let queue_succeeded = Family::<QueueLabels, Gauge<i64, AtomicI64>>::default();
        let queue_panicked = Family::<QueueLabels, Gauge<i64, AtomicI64>>::default();
        let queue_failed = Family::<QueueLabels, Gauge<i64, AtomicI64>>::default();
        let queue_latency_seconds = Family::<QueueLabels, Gauge<f64, AtomicU64>>::default();

        // Dynamic sub-queue metrics
        let dynamic_queue_enqueued = Family::<DynamicQueueLabels, Gauge<i64, AtomicI64>>::default();
        let dynamic_queue_processed =
            Family::<DynamicQueueLabels, Gauge<i64, AtomicI64>>::default();
        let dynamic_queue_succeeded =
            Family::<DynamicQueueLabels, Gauge<i64, AtomicI64>>::default();
        let dynamic_queue_panicked = Family::<DynamicQueueLabels, Gauge<i64, AtomicI64>>::default();
        let dynamic_queue_failed = Family::<DynamicQueueLabels, Gauge<i64, AtomicI64>>::default();
        let dynamic_queue_latency_seconds =
            Family::<DynamicQueueLabels, Gauge<f64, AtomicU64>>::default();

        // Set queue values
        for queue_stats in &stats.queues {
            let labels = QueueLabels {
                queue: queue_stats.key.clone(),
            };

            queue_enqueued
                .get_or_create(&labels)
                .set(queue_stats.enqueued as i64);
            queue_processed
                .get_or_create(&labels)
                .set(queue_stats.processed);
            queue_succeeded
                .get_or_create(&labels)
                .set(queue_stats.succeeded);
            queue_panicked
                .get_or_create(&labels)
                .set(queue_stats.panicked);
            queue_failed.get_or_create(&labels).set(queue_stats.failed);
            queue_latency_seconds
                .get_or_create(&labels)
                .set(queue_stats.latency_s);

            // Set dynamic sub-queue values
            for dyn_stats in &queue_stats.queues {
                let dyn_labels = DynamicQueueLabels {
                    queue: queue_stats.key.clone(),
                    suffix: dyn_stats.suffix.clone(),
                };

                dynamic_queue_enqueued
                    .get_or_create(&dyn_labels)
                    .set(dyn_stats.enqueued as i64);
                dynamic_queue_processed
                    .get_or_create(&dyn_labels)
                    .set(dyn_stats.processed);
                dynamic_queue_succeeded
                    .get_or_create(&dyn_labels)
                    .set(dyn_stats.succeeded);
                dynamic_queue_panicked
                    .get_or_create(&dyn_labels)
                    .set(dyn_stats.panicked);
                dynamic_queue_failed
                    .get_or_create(&dyn_labels)
                    .set(dyn_stats.failed);
                dynamic_queue_latency_seconds
                    .get_or_create(&dyn_labels)
                    .set(dyn_stats.latency_s);
            }
        }

        // Register queue metrics
        registry.register(
            "queue_enqueued",
            "Number of jobs enqueued per queue",
            queue_enqueued,
        );
        registry.register(
            "queue_processed_total",
            "Total number of jobs processed per queue",
            queue_processed,
        );
        registry.register(
            "queue_succeeded_total",
            "Total number of jobs succeeded per queue",
            queue_succeeded,
        );
        registry.register(
            "queue_panicked_total",
            "Total number of jobs panicked per queue",
            queue_panicked,
        );
        registry.register(
            "queue_failed_total",
            "Total number of jobs failed per queue",
            queue_failed,
        );
        registry.register(
            "queue_latency_seconds",
            "Current latency per queue in seconds",
            queue_latency_seconds,
        );

        // Register dynamic queue metrics
        registry.register(
            "dynamic_queue_enqueued",
            "Number of jobs enqueued per dynamic sub-queue",
            dynamic_queue_enqueued,
        );
        registry.register(
            "dynamic_queue_processed_total",
            "Total number of jobs processed per dynamic sub-queue",
            dynamic_queue_processed,
        );
        registry.register(
            "dynamic_queue_succeeded_total",
            "Total number of jobs succeeded per dynamic sub-queue",
            dynamic_queue_succeeded,
        );
        registry.register(
            "dynamic_queue_panicked_total",
            "Total number of jobs panicked per dynamic sub-queue",
            dynamic_queue_panicked,
        );
        registry.register(
            "dynamic_queue_failed_total",
            "Total number of jobs failed per dynamic sub-queue",
            dynamic_queue_failed,
        );
        registry.register(
            "dynamic_queue_latency_seconds",
            "Current latency per dynamic sub-queue in seconds",
            dynamic_queue_latency_seconds,
        );

        // Process metrics
        let process_heartbeat_timestamp = Family::<ProcessLabels, Gauge<i64, AtomicI64>>::default();
        let process_started_timestamp = Family::<ProcessLabels, Gauge<i64, AtomicI64>>::default();

        let processes_count = Gauge::<i64, AtomicI64>::default();
        processes_count.set(stats.processes.len() as i64);

        for process in &stats.processes {
            let labels = ProcessLabels {
                hostname: process.hostname.clone(),
                pid: process.pid.to_string(),
            };

            process_heartbeat_timestamp
                .get_or_create(&labels)
                .set(process.heartbeat_at);
            process_started_timestamp
                .get_or_create(&labels)
                .set(process.started_at);
        }

        registry.register(
            "process_heartbeat_timestamp_seconds",
            "Last heartbeat timestamp per process",
            process_heartbeat_timestamp,
        );
        registry.register(
            "process_started_timestamp_seconds",
            "Start timestamp per process",
            process_started_timestamp,
        );
        registry.register(
            "processes_count",
            "Number of active Oxanus processes",
            processes_count,
        );

        Self { registry }
    }

    /// Returns a reference to the underlying Prometheus registry.
    ///
    /// This can be used for custom encoding or to add additional metrics.
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Encodes the metrics to the `OpenMetrics` text format.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding fails.
    pub fn encode(&self, writer: &mut String) -> Result<(), std::fmt::Error> {
        encode(writer, &self.registry)
    }

    /// Encodes the metrics and returns them as a string.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding fails.
    pub fn encode_to_string(&self) -> Result<String, std::fmt::Error> {
        let mut buffer = String::new();
        self.encode(&mut buffer)?;
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{DynamicQueueStats, Process, QueueStats, Stats, StatsGlobal};

    fn create_test_stats() -> Stats {
        Stats {
            global: StatsGlobal {
                jobs: 100,
                enqueued: 50,
                processed: 200,
                failed: 10,
                dead: 5,
                scheduled: 30,
                retries: 10,
                latency_s_max: 2.5,
            },
            processes: vec![Process {
                hostname: "test-host".to_string(),
                pid: 12345,
                heartbeat_at: 1700000000,
                started_at: 1699999000,
            }],
            processing: vec![],
            queues: vec![
                QueueStats {
                    key: "default".to_string(),
                    enqueued: 30,
                    processed: 150,
                    succeeded: 140,
                    panicked: 2,
                    failed: 8,
                    latency_s: 1.5,
                    queues: vec![],
                },
                QueueStats {
                    key: "priority".to_string(),
                    enqueued: 20,
                    processed: 50,
                    succeeded: 48,
                    panicked: 0,
                    failed: 2,
                    latency_s: 0.5,
                    queues: vec![DynamicQueueStats {
                        suffix: "user_123".to_string(),
                        enqueued: 5,
                        processed: 10,
                        succeeded: 9,
                        panicked: 0,
                        failed: 1,
                        latency_s: 0.3,
                    }],
                },
            ],
        }
    }

    #[test]
    fn test_prometheus_metrics_from_stats() {
        let stats = create_test_stats();
        let metrics = PrometheusMetrics::from_stats(&stats);

        // Verify metrics can be encoded
        let output = metrics.encode_to_string().expect("encoding should succeed");
        assert!(!output.is_empty());
    }

    #[test]
    fn test_prometheus_metrics_with_prefix() {
        let stats = create_test_stats();
        let metrics = PrometheusMetrics::from_stats_with_prefix(&stats, "my_app");
        let output = metrics.encode_to_string().expect("encoding should succeed");
        assert!(output.contains("my_app_"));
    }

    #[test]
    fn test_prometheus_metrics_encode() {
        let stats = create_test_stats();
        let metrics = PrometheusMetrics::from_stats(&stats);

        let output = metrics.encode_to_string().expect("encoding should succeed");

        // Check that metrics are present in the output
        assert!(output.contains("oxanus_jobs_total"));
        assert!(output.contains("oxanus_enqueued_total"));
        assert!(output.contains("oxanus_processed_total"));
        assert!(output.contains("oxanus_failed_total"));
        assert!(output.contains("oxanus_dead_total"));
        assert!(output.contains("oxanus_scheduled_total"));
        assert!(output.contains("oxanus_retries_total"));
        assert!(output.contains("oxanus_queue_enqueued"));
        assert!(output.contains("oxanus_processes_count"));

        // Check that queue labels are present
        assert!(output.contains("queue=\"default\""));
        assert!(output.contains("queue=\"priority\""));

        // Check that dynamic queue labels are present
        assert!(output.contains("suffix=\"user_123\""));

        // Check that process labels are present
        assert!(output.contains("hostname=\"test-host\""));
        assert!(output.contains("pid=\"12345\""));
    }
}
