use axum::{
    extract::{Extension, Path, Query},
    response::Redirect,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::JOBS_PER_PAGE;
use crate::OxanusWebState;
use crate::error::OxanusWebError;
use crate::templates::{
    BusyTemplate, CronRow, CronTemplate, CronWorkerView, DashboardTemplate, GlobalJobsTemplate,
    JobListKind, QueueDetailTemplate, QueuesTemplate,
};

pub(crate) async fn dashboard(
    Extension(state): Extension<OxanusWebState>,
) -> Result<DashboardTemplate, OxanusWebError> {
    let stats = state.storage.stats().await?;
    let concurrency_map = build_concurrency_map(&state);

    Ok(DashboardTemplate {
        base_path: state.base_path,
        active_tab: "",
        stats,
        concurrency_map,
    })
}

pub(crate) async fn busy(
    Extension(state): Extension<OxanusWebState>,
) -> Result<BusyTemplate, OxanusWebError> {
    let stats = state.storage.stats().await?;
    let concurrency_map = build_concurrency_map(&state);

    Ok(BusyTemplate {
        base_path: state.base_path,
        active_tab: "/busy",
        stats,
        concurrency_map,
    })
}

pub(crate) async fn queues_list(
    Extension(state): Extension<OxanusWebState>,
    Query(params): Query<QueuesParams>,
) -> Result<QueuesTemplate, OxanusWebError> {
    let mut stats = state.storage.stats().await?;
    let concurrency_map = build_concurrency_map(&state);

    let sort = params.sort.as_deref().unwrap_or("key");
    let dir = params.dir.as_deref().unwrap_or("asc");
    let desc = dir == "desc";

    sort_queues(&mut stats.queues, &concurrency_map, sort, desc);

    Ok(QueuesTemplate {
        base_path: state.base_path,
        active_tab: "/queues",
        queues: stats.queues,
        concurrency_map,
        sort: sort.to_string(),
        dir: dir.to_string(),
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

    let total = state
        .storage
        .enqueued_count(RawQueue(queue_key.clone()))
        .await?;

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

// --- Helpers ---

fn build_concurrency_map(state: &OxanusWebState) -> HashMap<String, usize> {
    state
        .catalog
        .queues
        .iter()
        .map(|q| (q.key.clone(), q.concurrency))
        .collect()
}

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
        let cmp = match sort {
            "enqueued" => a.enqueued.cmp(&b.enqueued),
            "processed" => a.processed.cmp(&b.processed),
            "succeeded" => a.succeeded.cmp(&b.succeeded),
            "failed" => a.failed.cmp(&b.failed),
            "panicked" => a.panicked.cmp(&b.panicked),
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
    });
}

fn build_cron_rows(
    cron_workers: &[oxanus::CronWorkerInfo],
    now: &chrono::DateTime<chrono::Utc>,
) -> Vec<CronRow> {
    use std::collections::BTreeMap;

    struct TreeNode {
        children: BTreeMap<String, TreeNode>,
        workers: Vec<CronWorkerView>,
    }

    impl TreeNode {
        fn new() -> Self {
            Self {
                children: BTreeMap::new(),
                workers: Vec::new(),
            }
        }
    }

    let mut root = TreeNode::new();

    for cw in cron_workers {
        let segments: Vec<&str> = cw.name.split("::").collect();

        let split = segments.len().saturating_sub(2);
        let group_segments = segments.get(..split).unwrap_or_default();
        let leaf_name = segments.get(split..).unwrap_or_default().join("::");

        let mut node = &mut root;
        for seg in group_segments {
            node = node
                .children
                .entry((*seg).to_string())
                .or_insert_with(TreeNode::new);
        }

        node.workers.push(CronWorkerView {
            short_name: leaf_name,
            schedule: cw.schedule.to_string(),
            queue_key: cw.queue_key.clone(),
            next_run_micros: cw
                .schedule
                .after(now)
                .next()
                .map(|dt| dt.timestamp_micros()),
        });
    }

    fn flatten(node: &TreeNode, depth: usize, rows: &mut Vec<CronRow>) {
        for (name, child) in &node.children {
            rows.push(CronRow::Group {
                name: name.clone(),
                depth,
            });
            flatten(child, depth + 1, rows);
        }
        for worker in &node.workers {
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
