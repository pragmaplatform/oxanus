use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::any::type_name;
use uuid::Uuid;

use crate::{OxanusError, Worker};

pub type JobId = String;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobEnvelope {
    pub id: JobId,
    pub job: Job,
    pub queue: String,
    pub meta: JobMeta,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Job {
    pub name: String,
    pub args: serde_json::Value,
}

fn default_resurrect() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JobMeta {
    pub id: JobId,
    pub retries: u32,
    pub unique: bool,
    pub on_conflict: Option<JobConflictStrategy>,
    pub created_at: i64,
    #[serde(default)]
    pub scheduled_at: i64,
    pub state: Option<serde_json::Value>,
    #[serde(default = "default_resurrect")]
    pub resurrect: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobConflictStrategy {
    #[default]
    Skip,
    Replace,
}

impl JobEnvelope {
    pub(crate) fn new<T, DT, ET>(queue: String, job: T) -> Result<Self, OxanusError>
    where
        T: Worker<Context = DT, Error = ET> + serde::Serialize,
        DT: Send + Sync + Clone + 'static,
        ET: std::error::Error + Send + Sync + 'static,
    {
        let job_name = type_name::<T>().to_string();
        let unique_id = job.unique_id();
        let unique = unique_id.is_some();
        let resurrect = T::should_resurrect();
        let id = match unique_id {
            Some(id) => format!("{}/{}", job_name, id),
            None => Uuid::new_v4().to_string(),
        };
        Ok(Self {
            id: id.clone(),
            queue,
            job: Job {
                name: job_name,
                args: serde_json::to_value(&job)?,
            },
            meta: JobMeta {
                id,
                retries: 0,
                unique,
                on_conflict: if unique {
                    Some(job.on_conflict())
                } else {
                    None
                },
                created_at: chrono::Utc::now().timestamp_micros(),
                scheduled_at: chrono::Utc::now().timestamp_micros(),
                state: None,
                resurrect,
            },
        })
    }

    pub(crate) fn new_cron(
        queue: String,
        id: String,
        name: String,
        scheduled_at: i64,
        resurrect: bool,
    ) -> Result<Self, OxanusError> {
        Ok(Self {
            id: id.clone(),
            queue,
            job: Job {
                name,
                args: serde_json::to_value(serde_json::json!({}))?,
            },
            meta: JobMeta {
                id,
                retries: 0,
                unique: true,
                on_conflict: Some(JobConflictStrategy::Skip),
                created_at: chrono::Utc::now().timestamp_micros(),
                scheduled_at,
                state: None,
                resurrect,
            },
        })
    }

    pub(crate) fn with_retries_incremented(self) -> Self {
        Self {
            id: self.id.clone(),
            queue: self.queue,
            job: self.job,
            meta: JobMeta {
                id: self.id,
                retries: self.meta.retries + 1,
                unique: self.meta.unique,
                on_conflict: self.meta.on_conflict,
                created_at: self.meta.created_at,
                scheduled_at: self.meta.scheduled_at,
                state: self.meta.state,
                resurrect: self.meta.resurrect,
            },
        }
    }
}

impl JobMeta {
    pub fn created_at_secs(&self) -> i64 {
        self.created_at / 1000000
    }

    pub fn created_at_millis(&self) -> i64 {
        self.created_at / 1000
    }

    pub fn scheduled_at_millis(&self) -> i64 {
        self.scheduled_at / 1000
    }

    pub fn scheduled_at_secs(&self) -> i64 {
        self.scheduled_at / 1000000
    }

    pub fn latency_micros(&self) -> i64 {
        (chrono::Utc::now().timestamp_micros() - self.scheduled_at).max(0)
    }

    pub fn latency_secs(&self) -> i64 {
        self.latency_micros() / 1000000
    }

    pub fn latency_millis(&self) -> i64 {
        self.latency_micros() / 1000
    }

    pub fn scheduled_at(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp_micros(self.scheduled_at).unwrap_or_else(Utc::now)
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp_micros(self.created_at).unwrap_or_else(Utc::now)
    }
}
