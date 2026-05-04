use axum::{
    Form,
    extract::{Extension, Path, Query},
    response::Redirect,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::JOBS_PER_PAGE;
use crate::OxanusWebState;
use crate::error::OxanusWebError;
use crate::templates::{
    BusyTemplate, CronRow, CronTemplate, CronWorkerView, DashboardTemplate, GlobalJobsTemplate,
    JobListKind, MetricDetailTemplate, MetricsTemplate, OnDemandJobView, OnDemandQueueView,
    OnDemandRow, OnDemandTemplate, QueueDetailTemplate, QueuesTemplate,
};

pub(crate) async fn dashboard(
    Extension(state): Extension<OxanusWebState>,
) -> Result<DashboardTemplate, OxanusWebError> {
    let stats = state.storage.stats().await?;

    Ok(DashboardTemplate {
        base_path: state.base_path,
        active_tab: "",
        stats,
        concurrency_map: state.concurrency_map,
    })
}

pub(crate) async fn busy(
    Extension(state): Extension<OxanusWebState>,
) -> Result<BusyTemplate, OxanusWebError> {
    let stats = state.storage.stats().await?;

    Ok(BusyTemplate {
        base_path: state.base_path,
        active_tab: "/busy",
        stats,
        concurrency_map: state.concurrency_map,
    })
}

pub(crate) async fn queues_list(
    Extension(state): Extension<OxanusWebState>,
    Query(params): Query<QueuesParams>,
) -> Result<QueuesTemplate, OxanusWebError> {
    let mut stats = state.storage.stats().await?;
    let query = oxanus::JobMetricsQuery::new(params.minutes.unwrap_or(0));
    let queue_lengths = state.storage.queue_length_metrics(query).await?;

    let sort = params.sort.as_deref().unwrap_or("key");
    let dir = params.dir.as_deref().unwrap_or("asc");
    let desc = dir == "desc";

    sort_queues(&mut stats.queues, &state.concurrency_map, sort, desc);

    Ok(QueuesTemplate {
        base_path: state.base_path,
        active_tab: "/queues",
        stats,
        concurrency_map: state.concurrency_map,
        queue_lengths,
        sort: sort.to_string(),
        dir: dir.to_string(),
    })
}

pub(crate) async fn metrics(
    Extension(state): Extension<OxanusWebState>,
    Query(params): Query<MetricsParams>,
) -> Result<MetricsTemplate, OxanusWebError> {
    let query = oxanus::JobMetricsQuery::new(params.minutes.unwrap_or(0));
    let metrics = state.storage.job_metrics(query).await?;

    Ok(MetricsTemplate {
        base_path: state.base_path,
        active_tab: "/metrics",
        metrics,
    })
}

pub(crate) async fn metric_detail(
    Extension(state): Extension<OxanusWebState>,
    Query(params): Query<MetricDetailParams>,
) -> Result<MetricDetailTemplate, OxanusWebError> {
    let identity = oxanus::MetricIdentity {
        worker: params.worker,
    };
    let metrics = state
        .storage
        .job_metrics_for(
            &identity,
            oxanus::JobMetricsQuery::new(params.minutes.unwrap_or(0)),
        )
        .await?;

    Ok(MetricDetailTemplate {
        base_path: state.base_path,
        active_tab: "/metrics",
        metrics,
    })
}

pub(crate) async fn cron_jobs(Extension(state): Extension<OxanusWebState>) -> CronTemplate {
    let now = chrono::Utc::now();
    let rows = build_cron_rows(&state.catalog.cron_workers, &now);
    let total = state.catalog.cron_workers.len();

    CronTemplate {
        base_path: state.base_path,
        active_tab: "/cron",
        rows,
        total,
    }
}

pub(crate) async fn on_demand_jobs(
    Extension(state): Extension<OxanusWebState>,
    Query(params): Query<OnDemandParams>,
) -> OnDemandTemplate {
    let rows = build_on_demand_rows(&state.catalog.on_demand_jobs);
    let queues = state
        .catalog
        .queues
        .iter()
        .filter(|queue| !queue.dynamic)
        .map(|queue| OnDemandQueueView {
            key: queue.key.clone(),
        })
        .collect::<Vec<_>>();

    OnDemandTemplate {
        base_path: state.base_path,
        active_tab: "/on-demand",
        total: state.catalog.on_demand_jobs.len(),
        rows,
        queues,
        scheduled: params.scheduled.as_deref() == Some("1"),
        invalid_json: params.invalid_json.as_deref() == Some("1"),
    }
}

pub(crate) async fn enqueue_on_demand_job(
    Extension(state): Extension<OxanusWebState>,
    Form(form): Form<OnDemandEnqueueJobForm>,
) -> Result<Redirect, OxanusWebError> {
    let envelope = match on_demand_envelope_from_form(&state.catalog, &form) {
        Ok(envelope) => envelope,
        Err(oxanus::OxanusError::JsonError(_)) => {
            return Ok(Redirect::to(&format!(
                "{}/on-demand?invalid_json=1",
                state.base_path
            )));
        }
        Err(error) => return Err(error.into()),
    };

    state.storage.enqueue_envelope(envelope).await?;

    Ok(Redirect::to(&format!(
        "{}/on-demand?scheduled=1",
        state.base_path
    )))
}

pub(crate) async fn scheduled_jobs(
    Extension(state): Extension<OxanusWebState>,
    Query(params): Query<PaginationParams>,
) -> Result<GlobalJobsTemplate, OxanusWebError> {
    let page = params.page.max(1);
    let opts = list_opts(page);

    let total = state.storage.scheduled_count().await?;
    let mut jobs = state.storage.list_scheduled(&opts).await?;

    let has_next = jobs.len() > JOBS_PER_PAGE;
    jobs.truncate(JOBS_PER_PAGE);

    Ok(GlobalJobsTemplate {
        base_path: state.base_path,
        active_tab: "/scheduled",
        kind: JobListKind::Scheduled,
        jobs,
        page,
        total,
        has_next,
    })
}

pub(crate) async fn dead_jobs(
    Extension(state): Extension<OxanusWebState>,
    Query(params): Query<PaginationParams>,
) -> Result<GlobalJobsTemplate, OxanusWebError> {
    let page = params.page.max(1);
    let opts = list_opts(page);

    let total = state.storage.dead_count().await?;
    let mut jobs = state.storage.list_dead(&opts).await?;

    let has_next = jobs.len() > JOBS_PER_PAGE;
    jobs.truncate(JOBS_PER_PAGE);

    Ok(GlobalJobsTemplate {
        base_path: state.base_path,
        active_tab: "/dead",
        kind: JobListKind::Dead,
        jobs,
        page,
        total,
        has_next,
    })
}

pub(crate) async fn retry_jobs(
    Extension(state): Extension<OxanusWebState>,
    Query(params): Query<PaginationParams>,
) -> Result<GlobalJobsTemplate, OxanusWebError> {
    let page = params.page.max(1);
    let opts = list_opts(page);

    let total = state.storage.retries_count().await?;
    let mut jobs = state.storage.list_retries(&opts).await?;

    let has_next = jobs.len() > JOBS_PER_PAGE;
    jobs.truncate(JOBS_PER_PAGE);

    Ok(GlobalJobsTemplate {
        base_path: state.base_path,
        active_tab: "/retries",
        kind: JobListKind::Retries,
        jobs,
        page,
        total,
        has_next,
    })
}

pub(crate) async fn queue_detail(
    Extension(state): Extension<OxanusWebState>,
    Path(queue_key): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<QueueDetailTemplate, OxanusWebError> {
    let page = params.page.max(1);
    let opts = list_opts(page);

    let stats = state.storage.stats().await?;
    let queue_stats = stats
        .queues
        .iter()
        .find(|q| q.key == queue_key)
        .cloned()
        .or_else(|| {
            // For dynamic sub-queues (prefix#suffix), look inside parent's sub-queues
            let (prefix, suffix) = queue_key.split_once('#')?;
            let parent = stats.queues.iter().find(|q| q.key == prefix)?;
            let dq = parent.queues.iter().find(|dq| dq.suffix == suffix)?;
            Some(oxanus::QueueStats {
                key: queue_key.clone(),
                enqueued: dq.enqueued,
                processed: dq.processed,
                succeeded: dq.succeeded,
                panicked: dq.panicked,
                failed: dq.failed,
                latency_s: dq.latency_s,
                rate: dq.rate,
                queues: Vec::new(),
            })
        });
    let total = queue_stats.as_ref().map_or(0, |q| q.enqueued);
    let busy = stats
        .processing
        .iter()
        .filter(|p| p.job_envelope.queue == queue_key)
        .count();

    let mut jobs = state
        .storage
        .list_queue_jobs(RawQueue(queue_key.clone()), &opts)
        .await?;

    let has_next = jobs.len() > JOBS_PER_PAGE;
    jobs.truncate(JOBS_PER_PAGE);

    Ok(QueueDetailTemplate {
        base_path: state.base_path,
        active_tab: "/queues",
        queue_key,
        queue_stats,
        busy,
        jobs,
        page,
        total,
        has_next,
    })
}

pub(crate) async fn wipe_queue(
    Extension(state): Extension<OxanusWebState>,
    Path(queue_key): Path<String>,
) -> Result<Redirect, OxanusWebError> {
    state
        .storage
        .wipe_queue(RawQueue(queue_key.clone()))
        .await?;

    Ok(Redirect::to(&format!(
        "{}/queues/{}",
        state.base_path,
        urlencoding::encode(&queue_key)
    )))
}

pub(crate) async fn delete_job(
    Extension(state): Extension<OxanusWebState>,
    Path((queue_key, job_id)): Path<(String, String)>,
) -> Result<Redirect, OxanusWebError> {
    state.storage.delete_job(&job_id).await?;

    Ok(Redirect::to(&format!(
        "{}/queues/{}",
        state.base_path,
        urlencoding::encode(&queue_key)
    )))
}

pub(crate) async fn enqueue_job(
    Extension(state): Extension<OxanusWebState>,
    Form(form): Form<EnqueueJobForm>,
) -> Result<Redirect, OxanusWebError> {
    let now = chrono::Utc::now().timestamp_micros();
    let id = uuid::Uuid::new_v4().to_string();
    let args: serde_json::Value =
        serde_json::from_str(&form.args).map_err(oxanus::OxanusError::from)?;
    let job_state: Option<serde_json::Value> = match form.state.as_deref() {
        Some(s) if !s.is_empty() => {
            Some(serde_json::from_str(s).map_err(oxanus::OxanusError::from)?)
        }
        _ => None,
    };

    let envelope = oxanus::JobEnvelope {
        id: id.clone(),
        queue: form.queue.clone(),
        job: oxanus::JobData {
            name: form.name,
            args,
        },
        meta: oxanus::JobMeta {
            id,
            retries: 0,
            unique: false,
            on_conflict: None,
            created_at: now,
            scheduled_at: now,
            started_at: None,
            state: job_state,
            resurrect: true,
            error: None,
            throttle_cost: None,
        },
    };

    state.storage.enqueue_envelope(envelope).await?;

    let redirect = form.redirect.as_deref().unwrap_or("/dead");
    Ok(Redirect::to(&format!("{}{}", state.base_path, redirect)))
}

// --- Helpers ---

#[derive(Serialize)]
struct RawQueue(String);

impl oxanus::Queue for RawQueue {
    fn key(&self) -> String {
        self.0.clone()
    }

    fn to_config() -> oxanus::QueueConfig {
        oxanus::QueueConfig::as_static("")
    }
}

#[derive(Deserialize)]
pub(crate) struct PaginationParams {
    #[serde(default = "default_page")]
    page: usize,
}

fn default_page() -> usize {
    1
}

#[derive(Deserialize)]
pub(crate) struct QueuesParams {
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    minutes: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct MetricsParams {
    #[serde(default)]
    minutes: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct MetricDetailParams {
    worker: String,
    #[serde(default)]
    minutes: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct EnqueueJobForm {
    queue: String,
    name: String,
    args: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    redirect: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct OnDemandEnqueueJobForm {
    queue: String,
    name: String,
    args: String,
}

#[derive(Deserialize)]
pub(crate) struct OnDemandParams {
    #[serde(default)]
    scheduled: Option<String>,
    #[serde(default)]
    invalid_json: Option<String>,
}

fn list_opts(page: usize) -> oxanus::QueueListOpts {
    oxanus::QueueListOpts {
        count: JOBS_PER_PAGE + 1,
        offset: (page - 1) * JOBS_PER_PAGE,
    }
}

fn sort_queues(
    queues: &mut [oxanus::QueueStats],
    concurrency_map: &HashMap<String, usize>,
    sort: &str,
    desc: bool,
) {
    queues.sort_by(|a, b| {
        if sort == "eta" {
            compare_eta(a.rate.eta_s, b.rate.eta_s, desc)
        } else {
            let cmp = match sort {
                "enqueued" => a.enqueued.cmp(&b.enqueued),
                "processed" => a.processed.cmp(&b.processed),
                "succeeded" => a.succeeded.cmp(&b.succeeded),
                "failed" => a.failed.cmp(&b.failed),
                "panicked" => a.panicked.cmp(&b.panicked),
                "rate" => a
                    .rate
                    .processed_per_minute
                    .partial_cmp(&b.rate.processed_per_minute)
                    .unwrap_or(std::cmp::Ordering::Equal),
                "concurrency" => {
                    let ca = concurrency_map.get(&a.key).copied().unwrap_or(0);
                    let cb = concurrency_map.get(&b.key).copied().unwrap_or(0);
                    ca.cmp(&cb)
                }
                "latency" => a
                    .latency_s
                    .partial_cmp(&b.latency_s)
                    .unwrap_or(std::cmp::Ordering::Equal),
                _ => a.key.cmp(&b.key),
            };
            if desc { cmp.reverse() } else { cmp }
        }
    });
}

fn compare_eta(a: Option<f64>, b: Option<f64>, desc: bool) -> std::cmp::Ordering {
    let cmp = match (a, b) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    };

    if desc { cmp.reverse() } else { cmp }
}

fn on_demand_envelope_from_form(
    catalog: &oxanus::Catalog,
    form: &OnDemandEnqueueJobForm,
) -> Result<oxanus::JobEnvelope, oxanus::OxanusError> {
    if !catalog
        .queues
        .iter()
        .any(|queue| !queue.dynamic && queue.key == form.queue)
    {
        return Err(oxanus::OxanusError::GenericError(format!(
            "Queue {} is not a registered static queue",
            form.queue
        )));
    }

    let job = catalog
        .on_demand_jobs
        .iter()
        .find(|job| job.name == form.name)
        .ok_or_else(|| {
            oxanus::OxanusError::GenericError(format!(
                "Job {} is not registered for on-demand enqueue",
                form.name
            ))
        })?;
    let args: serde_json::Value =
        serde_json::from_str(&form.args).map_err(oxanus::OxanusError::from)?;

    job.enqueue_envelope(form.queue.clone(), args)
}

struct TypeTreeNode<T> {
    children: BTreeMap<String, TypeTreeNode<T>>,
    entries: Vec<T>,
}

impl<T> TypeTreeNode<T> {
    fn new() -> Self {
        Self {
            children: BTreeMap::new(),
            entries: Vec::new(),
        }
    }
}

fn type_tree_parts(name: &str) -> (Vec<&str>, String) {
    let segments: Vec<&str> = name.split("::").collect();
    let split = segments.len().saturating_sub(2);

    (segments[..split].to_vec(), segments[split..].join("::"))
}

fn insert_type_tree_entry<T>(root: &mut TypeTreeNode<T>, group_segments: &[&str], entry: T) {
    let mut node = root;
    for seg in group_segments {
        node = node
            .children
            .entry((*seg).to_string())
            .or_insert_with(TypeTreeNode::new);
    }

    node.entries.push(entry);
}

fn build_cron_rows(
    cron_workers: &[oxanus::CronWorkerInfo],
    now: &chrono::DateTime<chrono::Utc>,
) -> Vec<CronRow> {
    let mut root = TypeTreeNode::new();

    for cw in cron_workers {
        let (group_segments, leaf_name) = type_tree_parts(&cw.name);
        insert_type_tree_entry(
            &mut root,
            &group_segments,
            CronWorkerView {
                short_name: leaf_name,
                schedule: cw.schedule.to_string(),
                queue_key: cw.queue_key.clone(),
                next_run_micros: cw
                    .schedule
                    .after(now)
                    .next()
                    .map(|dt| dt.timestamp_micros()),
            },
        );
    }

    fn flatten(node: &TypeTreeNode<CronWorkerView>, depth: usize, rows: &mut Vec<CronRow>) {
        for (name, child) in &node.children {
            rows.push(CronRow::Group {
                name: name.clone(),
                depth,
            });
            flatten(child, depth + 1, rows);
        }
        for worker in &node.entries {
            rows.push(CronRow::Worker {
                view: worker.clone(),
                depth,
            });
        }
    }

    let mut rows = Vec::new();
    flatten(&root, 0, &mut rows);
    rows
}

fn build_on_demand_rows(on_demand_jobs: &[oxanus::OnDemandJobInfo]) -> Vec<OnDemandRow> {
    let mut root = TypeTreeNode::new();

    for job in on_demand_jobs {
        let (group_segments, leaf_name) = type_tree_parts(&job.name);
        insert_type_tree_entry(
            &mut root,
            &group_segments,
            OnDemandJobView {
                name: job.name.clone(),
                short_name: leaf_name,
                args_template_json: serde_json::to_string_pretty(&job.args_template)
                    .unwrap_or_else(|_| job.args_template.to_string()),
            },
        );
    }

    fn flatten(node: &TypeTreeNode<OnDemandJobView>, depth: usize, rows: &mut Vec<OnDemandRow>) {
        for (name, child) in &node.children {
            rows.push(OnDemandRow::Group {
                name: name.clone(),
                depth,
            });
            flatten(child, depth + 1, rows);
        }
        for job in &node.entries {
            rows.push(OnDemandRow::Job {
                view: job.clone(),
                depth,
            });
        }
    }

    let mut rows = Vec::new();
    flatten(&root, 0, &mut rows);
    rows
}

#[cfg(test)]
mod tests {
    use super::{
        OnDemandEnqueueJobForm, enqueue_on_demand_job, on_demand_envelope_from_form, sort_queues,
    };
    use crate::OxanusWebState;
    use axum::Form;
    use axum::extract::Extension;
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::io::Error as WorkerError;

    #[derive(Serialize)]
    struct StaticQueue;

    impl oxanus::Queue for StaticQueue {
        fn to_config() -> oxanus::QueueConfig {
            oxanus::QueueConfig::as_static("default")
        }
    }

    #[derive(Serialize)]
    struct DynamicQueue {
        tenant: String,
    }

    impl oxanus::Queue for DynamicQueue {
        fn to_config() -> oxanus::QueueConfig {
            oxanus::QueueConfig::as_dynamic("tenant")
        }
    }

    #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
    #[oxanus(worker = OnDemandWorker)]
    #[oxanus(on_demand)]
    #[oxanus(unique_id = "on_demand_{id}")]
    struct OnDemandJob {
        id: u64,
        payload: String,
    }

    struct OnDemandWorker;

    impl oxanus::FromContext<()> for OnDemandWorker {
        fn from_context(_ctx: &()) -> Self {
            Self
        }
    }

    #[async_trait::async_trait]
    impl oxanus::Worker<OnDemandJob> for OnDemandWorker {
        type Error = WorkerError;

        async fn run_batch(
            &self,
            _jobs: Vec<oxanus::BatchItem<OnDemandJob>>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn on_demand_catalog() -> oxanus::Catalog {
        let storage = oxanus::Storage::builder()
            .build_from_redis_url("redis://127.0.0.1/0")
            .expect("test storage pool should build");

        oxanus::Config::<(), WorkerError>::new(&storage)
            .register_queue::<StaticQueue>()
            .register_queue::<DynamicQueue>()
            .register_worker::<OnDemandWorker, OnDemandJob>()
            .catalog()
    }

    fn queue_with_eta(key: &str, eta_s: Option<f64>) -> oxanus::QueueStats {
        oxanus::QueueStats {
            key: key.to_string(),
            enqueued: 0,
            processed: 0,
            succeeded: 0,
            panicked: 0,
            failed: 0,
            latency_s: 0.0,
            rate: oxanus::QueueRateStats {
                eta_s,
                ..oxanus::QueueRateStats::default()
            },
            queues: Vec::new(),
        }
    }

    #[test]
    fn eta_sort_ascending_places_unknown_last() {
        let mut queues = vec![
            queue_with_eta("never", None),
            queue_with_eta("slow", Some(30.0)),
            queue_with_eta("fast", Some(10.0)),
        ];

        sort_queues(&mut queues, &HashMap::new(), "eta", false);

        let keys = queues
            .iter()
            .map(|queue| queue.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["fast", "slow", "never"]);
    }

    #[test]
    fn eta_sort_descending_places_unknown_first() {
        let mut queues = vec![
            queue_with_eta("fast", Some(10.0)),
            queue_with_eta("never", None),
            queue_with_eta("slow", Some(30.0)),
        ];

        sort_queues(&mut queues, &HashMap::new(), "eta", true);

        let keys = queues
            .iter()
            .map(|queue| queue.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["never", "slow", "fast"]);
    }

    #[test]
    fn on_demand_form_rejects_dynamic_or_unknown_queues() {
        let catalog = on_demand_catalog();

        let err = on_demand_envelope_from_form(
            &catalog,
            &OnDemandEnqueueJobForm {
                queue: "tenant#tenant=acme".to_string(),
                name: std::any::type_name::<OnDemandWorker>().to_string(),
                args: serde_json::json!({
                    "id": 1,
                    "payload": "hello",
                })
                .to_string(),
            },
        )
        .expect_err("dynamic queues should not be accepted");

        assert!(matches!(err, oxanus::OxanusError::GenericError(_)));
    }

    #[test]
    fn on_demand_form_rejects_unknown_jobs() {
        let catalog = on_demand_catalog();

        let err = on_demand_envelope_from_form(
            &catalog,
            &OnDemandEnqueueJobForm {
                queue: "default".to_string(),
                name: "missing".to_string(),
                args: "{}".to_string(),
            },
        )
        .expect_err("unknown jobs should not be accepted");

        assert!(matches!(err, oxanus::OxanusError::GenericError(_)));
    }

    #[tokio::test]
    async fn on_demand_enqueue_redirects_after_invalid_json() {
        let storage = oxanus::Storage::builder()
            .build_from_redis_url("redis://127.0.0.1/0")
            .expect("test storage pool should build");
        let state = OxanusWebState::new(storage, on_demand_catalog(), "/admin".to_string());

        let redirect = enqueue_on_demand_job(
            Extension(state),
            Form(OnDemandEnqueueJobForm {
                queue: "default".to_string(),
                name: std::any::type_name::<OnDemandWorker>().to_string(),
                args: "{".to_string(),
            }),
        )
        .await
        .expect("invalid JSON should redirect back to on-demand");

        let response = redirect.into_response();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "/admin/on-demand?invalid_json=1"
        );
    }

    #[test]
    fn on_demand_form_builds_typed_envelope() {
        let catalog = on_demand_catalog();
        let envelope = on_demand_envelope_from_form(
            &catalog,
            &OnDemandEnqueueJobForm {
                queue: "default".to_string(),
                name: std::any::type_name::<OnDemandWorker>().to_string(),
                args: serde_json::json!({
                    "id": 42,
                    "payload": "hello",
                })
                .to_string(),
            },
        )
        .expect("valid on-demand form should build an envelope");

        assert_eq!(envelope.queue, "default");
        assert_eq!(
            envelope.id,
            format!("{}/on_demand_42", std::any::type_name::<OnDemandWorker>())
        );
        assert_eq!(
            envelope.job.args,
            serde_json::json!({
                "id": 42,
                "payload": "hello",
            })
        );
    }
}
