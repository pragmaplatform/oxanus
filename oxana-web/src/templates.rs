use askama::Template;
use askama_web::WebTemplate;
use std::collections::HashMap;

use crate::JOBS_PER_PAGE;
use crate::filters;

fn busy_for(stats: &oxana::Stats, key: &str) -> usize {
    stats
        .processing
        .iter()
        .filter(|p| p.job_envelope.queue == key)
        .count()
}

fn busy_for_process(stats: &oxana::Stats, process: &oxana::Process) -> usize {
    let id = process.id();
    stats
        .processing
        .iter()
        .filter(|p| p.process_id == id)
        .count()
}

fn concurrency_for(concurrency_map: &HashMap<String, usize>, key: &str) -> String {
    concurrency_map
        .get(key)
        .map_or_else(|| "—".to_string(), |c| c.to_string())
}

#[derive(Template, WebTemplate)]
#[template(path = "dashboard.html")]
pub(crate) struct DashboardTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub stats: oxana::Stats,
    pub concurrency_map: HashMap<String, usize>,
}

impl DashboardTemplate {
    pub fn concurrency_for(&self, key: &str) -> String {
        concurrency_for(&self.concurrency_map, key)
    }

    pub fn busy_for(&self, key: &str) -> usize {
        busy_for(&self.stats, key)
    }

    pub fn busy_for_process(&self, process: &oxana::Process) -> usize {
        busy_for_process(&self.stats, process)
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "busy.html")]
pub(crate) struct BusyTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub stats: oxana::Stats,
    pub concurrency_map: HashMap<String, usize>,
}

impl BusyTemplate {
    pub fn concurrency_for(&self, key: &str) -> String {
        concurrency_for(&self.concurrency_map, key)
    }

    pub fn busy_for(&self, key: &str) -> usize {
        busy_for(&self.stats, key)
    }

    pub fn busy_for_process(&self, process: &oxana::Process) -> usize {
        busy_for_process(&self.stats, process)
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "queues.html")]
pub(crate) struct QueuesTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub stats: oxana::Stats,
    pub concurrency_map: HashMap<String, usize>,
    pub queue_lengths: oxana::QueueLengthMetricsSnapshot,
    pub sort: String,
    pub dir: String,
}

impl QueuesTemplate {
    pub fn next_dir(&self, col: &str) -> &'static str {
        if self.sort == col {
            if self.dir == "desc" { "asc" } else { "desc" }
        } else {
            "desc"
        }
    }

    pub fn sort_arrow(&self, col: &str) -> &'static str {
        if self.sort == col {
            if self.dir == "asc" {
                " \u{25B2}"
            } else {
                " \u{25BC}"
            }
        } else {
            ""
        }
    }

    pub fn sort_href(&self, col: &str) -> String {
        format!(
            "{}/queues?sort={}&dir={}&minutes={}",
            self.base_path,
            col,
            self.next_dir(col),
            self.queue_lengths.minutes
        )
    }

    pub fn concurrency_for(&self, key: &str) -> String {
        concurrency_for(&self.concurrency_map, key)
    }

    pub fn queue_length_chart_data_json(&self) -> String {
        let timestamps: Vec<i64> = self.queue_lengths.queues.first().map_or_else(
            || {
                (0..self.queue_lengths.minutes)
                    .map(|idx| self.queue_lengths.starts_at + i64::try_from(idx).unwrap_or(0) * 60)
                    .collect()
            },
            |queue| queue.series.iter().map(|point| point.timestamp).collect(),
        );
        let series: Vec<serde_json::Value> = self
            .queue_lengths
            .queues
            .iter()
            .map(|queue| {
                let data: Vec<u64> = queue.series.iter().map(|point| point.enqueued).collect();
                serde_json::json!({
                    "label": queue.queue.clone(),
                    "data": data,
                })
            })
            .collect();

        serde_json::json!({
            "timestamps": timestamps,
            "series": series,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::QueuesTemplate;
    use std::collections::HashMap;

    fn queues_template(queue_lengths: oxana::QueueLengthMetricsSnapshot) -> QueuesTemplate {
        QueuesTemplate {
            base_path: "/admin".to_string(),
            active_tab: "/queues",
            stats: oxana::Stats {
                global: oxana::StatsGlobal {
                    jobs: 0,
                    enqueued: 0,
                    processed: 0,
                    failed: 0,
                    dead: 0,
                    scheduled: 0,
                    retries: 0,
                    latency_s_max: 0.0,
                },
                processes: Vec::new(),
                processing: Vec::new(),
                queues: Vec::new(),
            },
            concurrency_map: HashMap::new(),
            queue_lengths,
            sort: "key".to_string(),
            dir: "asc".to_string(),
        }
    }

    #[test]
    fn queue_sort_href_preserves_chart_window() {
        let template = queues_template(oxana::QueueLengthMetricsSnapshot {
            starts_at: 0,
            ends_at: 0,
            minutes: 120,
            queues: Vec::new(),
        });

        assert_eq!(
            template.sort_href("enqueued"),
            "/admin/queues?sort=enqueued&dir=desc&minutes=120"
        );
    }

    #[test]
    fn queue_length_chart_data_json_uses_window_when_no_queues_exist() {
        let template = queues_template(oxana::QueueLengthMetricsSnapshot {
            starts_at: 60,
            ends_at: 120,
            minutes: 2,
            queues: Vec::new(),
        });
        let payload: serde_json::Value =
            serde_json::from_str(&template.queue_length_chart_data_json()).unwrap();

        assert_eq!(payload["timestamps"], serde_json::json!([60, 120]));
        assert_eq!(payload["series"], serde_json::json!([]));
    }

    #[test]
    fn queue_length_chart_data_json_serializes_queue_series() {
        let template = queues_template(oxana::QueueLengthMetricsSnapshot {
            starts_at: 60,
            ends_at: 120,
            minutes: 2,
            queues: vec![oxana::QueueLengthMetricsSeries {
                queue: "default".to_string(),
                series: vec![
                    oxana::QueueLengthMetricsPoint {
                        timestamp: 60,
                        enqueued: 2,
                    },
                    oxana::QueueLengthMetricsPoint {
                        timestamp: 120,
                        enqueued: 5,
                    },
                ],
            }],
        });
        let payload: serde_json::Value =
            serde_json::from_str(&template.queue_length_chart_data_json()).unwrap();

        assert_eq!(payload["timestamps"], serde_json::json!([60, 120]));
        assert_eq!(
            payload["series"],
            serde_json::json!([{ "label": "default", "data": [2, 5] }])
        );
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "metrics.html")]
pub(crate) struct MetricsTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub metrics: oxana::JobMetricsSnapshot,
}

impl MetricsTemplate {
    pub fn metric_identity_label(&self, identity: &oxana::MetricIdentity) -> String {
        metric_identity_label(identity)
    }

    pub fn execution_chart_data_json(&self) -> String {
        self.summary_chart_data_json(|point| point.execution_ms as f64 / 1000.0)
    }

    pub fn processed_chart_data_json(&self) -> String {
        self.summary_chart_data_json(|point| point.processed as f64)
    }

    fn summary_chart_data_json(&self, value: impl Fn(&oxana::JobMetricsPoint) -> f64) -> String {
        let timestamps: Vec<i64> = self
            .metrics
            .series
            .iter()
            .map(|point| point.timestamp)
            .collect();
        let series: Vec<serde_json::Value> = self
            .metrics
            .workers
            .iter()
            .map(|worker| {
                let data: Vec<f64> = worker.series.iter().map(&value).collect();
                serde_json::json!({
                    "label": metric_identity_label(&worker.identity),
                    "fullLabel": worker.identity.worker,
                    "data": data,
                })
            })
            .collect();

        serde_json::json!({
            "timestamps": timestamps,
            "series": series,
        })
        .to_string()
    }
}

fn metric_identity_label(identity: &oxana::MetricIdentity) -> String {
    identity
        .worker
        .rsplit("::")
        .next()
        .unwrap_or(&identity.worker)
        .to_string()
}

#[derive(Template, WebTemplate)]
#[template(path = "metric_detail.html")]
pub(crate) struct MetricDetailTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub metrics: oxana::JobMetricsDetail,
}

impl MetricDetailTemplate {
    pub fn detail_average_chart_data_json(&self) -> String {
        let timestamps: Vec<i64> = self
            .metrics
            .series
            .iter()
            .map(|point| point.timestamp)
            .collect();
        let average_ms: Vec<f64> = self
            .metrics
            .series
            .iter()
            .map(oxana::JobMetricsPoint::average_execution_ms)
            .collect();

        serde_json::json!([timestamps, average_ms]).to_string()
    }

    pub fn detail_total_chart_data_json(&self) -> String {
        let timestamps: Vec<i64> = self
            .metrics
            .series
            .iter()
            .map(|point| point.timestamp)
            .collect();
        let succeeded: Vec<u64> = self
            .metrics
            .series
            .iter()
            .map(|point| point.successful_executions)
            .collect();
        let failed_without_panics: Vec<u64> = self
            .metrics
            .series
            .iter()
            .map(oxana::JobMetricsPoint::failed_executions_without_panics)
            .collect();
        let panicked: Vec<u64> = self
            .metrics
            .series
            .iter()
            .map(|point| point.panicked_executions)
            .collect();

        serde_json::json!([timestamps, succeeded, failed_without_panics, panicked]).to_string()
    }

    pub fn max_histogram_count(&self) -> u64 {
        self.metrics
            .histogram
            .iter()
            .map(|bucket| bucket.count)
            .max()
            .unwrap_or(0)
    }

    pub fn histogram_width(&self, count: &u64) -> String {
        let max = self.max_histogram_count();
        if max == 0 {
            "0%".to_string()
        } else {
            format!("{:.1}%", (*count as f64 / max as f64) * 100.0)
        }
    }
}

pub(crate) enum CronRow {
    Group { name: String, depth: usize },
    Worker { view: CronWorkerView, depth: usize },
}

impl CronRow {
    fn depth(&self) -> usize {
        match self {
            Self::Group { depth, .. } | Self::Worker { depth, .. } => *depth,
        }
    }

    pub fn indent_px(&self) -> usize {
        self.depth() * 20
    }
}

#[derive(Clone)]
pub(crate) struct CronWorkerView {
    pub name: String,
    pub short_name: String,
    pub schedule: String,
    pub queue_key: String,
    pub next_run_micros: Option<i64>,
}

#[derive(Template, WebTemplate)]
#[template(path = "cron.html")]
pub(crate) struct CronTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub rows: Vec<CronRow>,
    pub total: usize,
    pub enqueued: bool,
}

#[derive(Clone)]
pub(crate) struct OnDemandJobView {
    pub name: String,
    pub short_name: String,
    pub args_template_json: String,
}

pub(crate) enum OnDemandRow {
    Group { name: String, depth: usize },
    Job { view: OnDemandJobView, depth: usize },
}

impl OnDemandRow {
    fn depth(&self) -> usize {
        match self {
            Self::Group { depth, .. } | Self::Job { depth, .. } => *depth,
        }
    }

    pub fn indent_px(&self) -> usize {
        self.depth() * 20
    }
}

#[derive(Clone)]
pub(crate) struct OnDemandQueueView {
    pub key: String,
    pub selected: bool,
}

#[derive(Template, WebTemplate)]
#[template(path = "on_demand.html")]
pub(crate) struct OnDemandTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub rows: Vec<OnDemandRow>,
    pub queues: Vec<OnDemandQueueView>,
    pub total: usize,
    pub scheduled: bool,
    pub invalid_json: bool,
}

#[derive(Template, WebTemplate)]
#[template(path = "queue_detail.html")]
pub(crate) struct QueueDetailTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub queue_key: String,
    pub queue_stats: Option<oxana::QueueStats>,
    pub active_jobs: Vec<oxana::StatsProcessing>,
    pub busy: usize,
    pub jobs: Vec<oxana::JobEnvelope>,
    pub page: usize,
    pub total: usize,
    pub has_next: bool,
}

impl QueueDetailTemplate {
    pub fn range_start(&self) -> usize {
        (self.page - 1) * JOBS_PER_PAGE + 1
    }

    pub fn range_end(&self) -> usize {
        ((self.page - 1) * JOBS_PER_PAGE + self.jobs.len()).min(self.total)
    }
}

pub(crate) enum JobListKind {
    Dead,
    Retries,
    Scheduled,
}

impl JobListKind {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Dead => "Dead Jobs",
            Self::Retries => "Retries",
            Self::Scheduled => "Scheduled",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Dead => "dead jobs",
            Self::Retries => "retrying jobs",
            Self::Scheduled => "scheduled jobs",
        }
    }

    pub fn empty_label(&self) -> &'static str {
        match self {
            Self::Dead => "No dead jobs",
            Self::Retries => "No retrying jobs",
            Self::Scheduled => "No scheduled jobs",
        }
    }

    pub fn border_class(&self) -> &'static str {
        match self {
            Self::Dead => "border-red-900/50",
            Self::Retries => "border-orange-900/50",
            Self::Scheduled => "border-yellow-900/50",
        }
    }

    pub fn is_dead(&self) -> bool {
        matches!(self, Self::Dead)
    }

    pub fn dot_class(&self) -> &'static str {
        match self {
            Self::Dead => "bg-red-400",
            Self::Retries => "bg-orange-400",
            Self::Scheduled => "bg-yellow-400",
        }
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "global_jobs.html")]
pub(crate) struct GlobalJobsTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub kind: JobListKind,
    pub jobs: Vec<oxana::JobEnvelope>,
    pub page: usize,
    pub total: usize,
    pub has_next: bool,
}

impl GlobalJobsTemplate {
    pub fn range_start(&self) -> usize {
        (self.page - 1) * JOBS_PER_PAGE + 1
    }

    pub fn range_end(&self) -> usize {
        ((self.page - 1) * JOBS_PER_PAGE + self.jobs.len()).min(self.total)
    }
}

#[cfg(test)]
mod cron_tests {
    use super::{CronRow, CronTemplate, CronWorkerView};
    use askama::Template;

    #[test]
    fn cron_template_renders_enqueue_now_form_at_end_of_row() {
        let template = CronTemplate {
            base_path: "/admin".to_string(),
            active_tab: "/cron",
            rows: vec![CronRow::Worker {
                view: CronWorkerView {
                    name: "crate::workers::EmailCronWorker".to_string(),
                    short_name: "EmailCronWorker".to_string(),
                    schedule: "0 * * * * *".to_string(),
                    queue_key: "default".to_string(),
                    next_run_micros: None,
                },
                depth: 0,
            }],
            total: 1,
            enqueued: false,
        };

        let rendered = template.render().unwrap();

        assert!(rendered.contains("<th class=\"pb-3 text-right\">Action</th>"));
        assert!(rendered.contains("action=\"/admin/cron/enqueue\""));
        assert!(rendered.contains("name=\"name\" value=\"crate::workers::EmailCronWorker\""));
        assert!(rendered.contains("Enqueue now"));
    }

    #[test]
    fn cron_template_shows_notice_after_successful_enqueue() {
        let template = CronTemplate {
            base_path: "/admin".to_string(),
            active_tab: "/cron",
            rows: Vec::new(),
            total: 0,
            enqueued: true,
        };

        let rendered = template.render().unwrap();

        assert!(rendered.contains("Cron job enqueued."));
        assert!(rendered.contains("data-auto-dismiss-notice"));
    }
}

#[cfg(test)]
mod on_demand_tests {
    use super::{OnDemandJobView, OnDemandQueueView, OnDemandRow, OnDemandTemplate};
    use askama::Template;

    #[test]
    fn on_demand_template_renders_empty_state() {
        let template = OnDemandTemplate {
            base_path: "/admin".to_string(),
            active_tab: "/on-demand",
            rows: Vec::new(),
            queues: vec![OnDemandQueueView {
                key: "default".to_string(),
                selected: true,
            }],
            total: 0,
            scheduled: false,
            invalid_json: false,
        };

        let rendered = template.render().unwrap();

        assert!(rendered.contains("No on-demand jobs registered"));
    }

    #[test]
    fn on_demand_template_renders_no_static_queues_state() {
        let template = OnDemandTemplate {
            base_path: "/admin".to_string(),
            active_tab: "/on-demand",
            rows: vec![OnDemandRow::Job {
                view: OnDemandJobView {
                    name: "crate::EmailWorker".to_string(),
                    short_name: "EmailWorker".to_string(),
                    args_template_json: "{}".to_string(),
                },
                depth: 0,
            }],
            queues: Vec::new(),
            total: 1,
            scheduled: false,
            invalid_json: false,
        };

        let rendered = template.render().unwrap();

        assert!(rendered.contains("No static queues registered"));
    }

    #[test]
    fn on_demand_template_prefills_enqueue_form() {
        let template = OnDemandTemplate {
            base_path: "/admin".to_string(),
            active_tab: "/on-demand",
            rows: vec![
                OnDemandRow::Group {
                    name: "crate".to_string(),
                    depth: 0,
                },
                OnDemandRow::Job {
                    view: OnDemandJobView {
                        name: "crate::email::EmailWorker".to_string(),
                        short_name: "email::EmailWorker".to_string(),
                        args_template_json: "{\n  \"payload\": \"\"\n}".to_string(),
                    },
                    depth: 1,
                },
            ],
            queues: vec![OnDemandQueueView {
                key: "default".to_string(),
                selected: true,
            }],
            total: 1,
            scheduled: false,
            invalid_json: false,
        };

        let rendered = template.render().unwrap();

        assert!(rendered.contains("action=\"/admin/on-demand/enqueue\""));
        assert!(rendered.contains("<option value=\"default\" selected>default</option>"));
        assert!(rendered.contains("crate"));
        assert!(rendered.contains("email::EmailWorker"));
        assert!(rendered.contains("payload"));
    }

    #[test]
    fn on_demand_template_shows_notice_after_successful_enqueue() {
        let template = OnDemandTemplate {
            base_path: "/admin".to_string(),
            active_tab: "/on-demand",
            rows: Vec::new(),
            queues: Vec::new(),
            total: 0,
            scheduled: true,
            invalid_json: false,
        };

        let rendered = template.render().unwrap();

        assert!(rendered.contains("Job scheduled."));
        assert!(rendered.contains("data-auto-dismiss-notice"));
    }

    #[test]
    fn on_demand_template_shows_notice_after_invalid_json() {
        let template = OnDemandTemplate {
            base_path: "/admin".to_string(),
            active_tab: "/on-demand",
            rows: Vec::new(),
            queues: Vec::new(),
            total: 0,
            scheduled: false,
            invalid_json: true,
        };

        let rendered = template.render().unwrap();

        assert!(rendered.contains("Invalid JSON."));
        assert!(rendered.contains("data-auto-dismiss-notice"));
    }
}
