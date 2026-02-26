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
    #[serde(default)]
    pub started_at: Option<i64>,
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
                started_at: None,
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
                started_at: None,
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
                started_at: None,
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
        let reference = self
            .started_at
            .unwrap_or_else(|| chrono::Utc::now().timestamp_micros());
        (reference - self.scheduled_at).max(0)
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

    pub fn started_at(&self) -> Option<DateTime<Utc>> {
        self.started_at
            .and_then(DateTime::<Utc>::from_timestamp_micros)
    }

    pub fn started_at_secs(&self) -> Option<i64> {
        self.started_at.map(|t| t / 1000000)
    }

    pub fn started_at_millis(&self) -> Option<i64> {
        self.started_at.map(|t| t / 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_meta(scheduled_at: i64, started_at: Option<i64>) -> JobMeta {
        JobMeta {
            id: "test".to_string(),
            retries: 0,
            unique: false,
            on_conflict: None,
            created_at: scheduled_at,
            scheduled_at,
            started_at,
            state: None,
            resurrect: true,
        }
    }

    #[test]
    fn test_started_at_none() {
        let meta = make_meta(1_000_000, None);
        assert!(meta.started_at().is_none());
        assert!(meta.started_at_secs().is_none());
        assert!(meta.started_at_millis().is_none());
    }

    #[test]
    fn test_started_at_some() {
        let scheduled = 1_700_000_000_000_000i64;
        let started = scheduled + 5_500_000;
        let meta = make_meta(scheduled, Some(started));

        assert!(meta.started_at().is_some());
        assert_eq!(meta.started_at_secs(), Some(started / 1_000_000));
        assert_eq!(meta.started_at_millis(), Some(started / 1_000));
    }

    #[test]
    fn test_latency_uses_started_at_when_available() {
        let scheduled = 1_700_000_000_000_000i64;
        let started = scheduled + 5_500_000;
        let meta = make_meta(scheduled, Some(started));

        assert_eq!(meta.latency_micros(), 5_500_000);
        assert_eq!(meta.latency_millis(), 5_500);
        assert_eq!(meta.latency_secs(), 5);
    }

    #[test]
    fn test_latency_falls_back_to_now_without_started_at() {
        let scheduled = chrono::Utc::now().timestamp_micros() - 2_000_000;
        let meta = make_meta(scheduled, None);

        let latency = meta.latency_micros();
        assert!(latency >= 2_000_000);
        assert!(latency < 3_000_000);
    }

    #[test]
    fn test_latency_clamped_to_zero() {
        let scheduled = 1_700_000_000_000_000i64;
        let started = scheduled - 100;
        let meta = make_meta(scheduled, Some(started));

        assert_eq!(meta.latency_micros(), 0);
    }

    #[test]
    fn test_with_retries_incremented_resets_started_at() {
        let envelope = JobEnvelope {
            id: "test".to_string(),
            queue: "default".to_string(),
            job: Job {
                name: "TestJob".to_string(),
                args: serde_json::json!({}),
            },
            meta: JobMeta {
                id: "test".to_string(),
                retries: 0,
                unique: false,
                on_conflict: None,
                created_at: 1_000_000,
                scheduled_at: 1_000_000,
                started_at: Some(2_000_000),
                state: None,
                resurrect: true,
            },
        };

        let retried = envelope.with_retries_incremented();
        assert_eq!(retried.meta.retries, 1);
        assert!(retried.meta.started_at.is_none());
    }

    #[test]
    fn test_serde_backward_compatibility() {
        let json = r#"{
            "id": "test",
            "retries": 0,
            "unique": false,
            "on_conflict": null,
            "created_at": 1000000,
            "scheduled_at": 1000000,
            "state": null,
            "resurrect": true
        }"#;

        let meta: JobMeta =
            serde_json::from_str(json).expect("should deserialize without started_at");
        assert!(meta.started_at.is_none());
    }
}
