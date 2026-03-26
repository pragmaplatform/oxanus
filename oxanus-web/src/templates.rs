use askama::Template;
use askama_web::WebTemplate;
use std::collections::HashMap;

use crate::JOBS_PER_PAGE;
use crate::filters;

fn busy_for(stats: &oxanus::Stats, key: &str) -> usize {
    stats
        .processing
        .iter()
        .filter(|p| p.job_envelope.queue == key)
        .count()
}

fn busy_for_process(stats: &oxanus::Stats, process: &oxanus::Process) -> usize {
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
    pub stats: oxanus::Stats,
    pub concurrency_map: HashMap<String, usize>,
}

impl DashboardTemplate {
    pub fn concurrency_for(&self, key: &str) -> String {
        concurrency_for(&self.concurrency_map, key)
    }

    pub fn busy_for(&self, key: &str) -> usize {
        busy_for(&self.stats, key)
    }

    pub fn busy_for_process(&self, process: &oxanus::Process) -> usize {
        busy_for_process(&self.stats, process)
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "busy.html")]
pub(crate) struct BusyTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub stats: oxanus::Stats,
    pub concurrency_map: HashMap<String, usize>,
}

impl BusyTemplate {
    pub fn concurrency_for(&self, key: &str) -> String {
        concurrency_for(&self.concurrency_map, key)
    }

    pub fn busy_for(&self, key: &str) -> usize {
        busy_for(&self.stats, key)
    }

    pub fn busy_for_process(&self, process: &oxanus::Process) -> usize {
        busy_for_process(&self.stats, process)
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "queues.html")]
pub(crate) struct QueuesTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub queues: Vec<oxanus::QueueStats>,
    pub concurrency_map: HashMap<String, usize>,
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

    pub fn concurrency_for(&self, key: &str) -> String {
        concurrency_for(&self.concurrency_map, key)
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
}

#[derive(Template, WebTemplate)]
#[template(path = "queue_detail.html")]
pub(crate) struct QueueDetailTemplate {
    pub base_path: String,
    pub active_tab: &'static str,
    pub queue_key: String,
    pub jobs: Vec<oxanus::JobEnvelope>,
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
    pub jobs: Vec<oxanus::JobEnvelope>,
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
