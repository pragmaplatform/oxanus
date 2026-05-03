use std::collections::HashMap;
use std::str::FromStr;

use crate::WorkerBatchConfig;
use crate::error::OxanusError;
use crate::worker::{BoundBatchJob, BoundJob, BoxedProcessable, FromContext, Worker};

pub type JobFactory<DT, ET> =
    fn(serde_json::Value, &DT) -> Result<BoxedProcessable<ET>, OxanusError>;
pub type JobBatchFactory<DT, ET> =
    fn(Vec<serde_json::Value>, &DT) -> Result<BatchBuild<ET>, OxanusError>;

pub struct BatchBuild<ET> {
    pub job: Option<BoxedProcessable<ET>>,
    pub invalid: Vec<InvalidBatchJob>,
}

pub struct InvalidBatchJob {
    pub index: usize,
    pub error: String,
}

pub struct WorkerRegistry<DT, ET> {
    jobs: HashMap<String, WorkerFactories<DT, ET>>,
    pub schedules: HashMap<String, CronJob>,
}

pub struct WorkerConfig<DT, ET> {
    pub name: String,
    pub factory: JobFactory<DT, ET>,
    pub batch_factory: JobBatchFactory<DT, ET>,
    pub batch_config: Option<WorkerBatchConfig>,
    pub kind: WorkerConfigKind,
}

pub enum WorkerConfigKind {
    Normal,
    Cron {
        schedule: String,
        queue_key: String,
        resurrect: bool,
    },
}

#[derive(Debug, Clone)]
pub struct CronJob {
    pub schedule: cron::Schedule,
    pub queue_key: String,
    pub resurrect: bool,
}

pub fn job_factory<W, A, DT, ET>(
    value: serde_json::Value,
    ctx: &DT,
) -> Result<BoxedProcessable<ET>, OxanusError>
where
    W: Worker<A, Error = ET> + FromContext<DT> + 'static,
    A: serde::de::DeserializeOwned + Send + 'static,
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let job: A = serde_json::from_value(value)?;
    let worker = W::from_context(ctx);
    Ok(Box::new(BoundJob { worker, job }))
}

pub fn job_batch_factory<W, A, DT, ET>(
    values: Vec<serde_json::Value>,
    ctx: &DT,
) -> Result<BatchBuild<ET>, OxanusError>
where
    W: Worker<A, Error = ET> + FromContext<DT> + 'static,
    A: serde::de::DeserializeOwned + Send + 'static,
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let mut jobs = Vec::with_capacity(values.len());
    let mut invalid = Vec::new();

    for (index, value) in values.into_iter().enumerate() {
        match serde_json::from_value(value) {
            Ok(job) => jobs.push(job),
            Err(error) => invalid.push(InvalidBatchJob {
                index,
                error: error.to_string(),
            }),
        }
    }

    if jobs.is_empty() {
        return Ok(BatchBuild { job: None, invalid });
    }

    let worker = W::from_context(ctx);
    Ok(BatchBuild {
        job: Some(Box::new(BoundBatchJob { worker, jobs })),
        invalid,
    })
}

struct WorkerFactories<DT, ET> {
    factory: JobFactory<DT, ET>,
    batch_factory: JobBatchFactory<DT, ET>,
    batch_config: Option<WorkerBatchConfig>,
}

impl<DT, ET> WorkerRegistry<DT, ET> {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            schedules: HashMap::new(),
        }
    }

    pub fn register_worker_with(&mut self, config: WorkerConfig<DT, ET>) {
        let factories = WorkerFactories {
            factory: config.factory,
            batch_factory: config.batch_factory,
            batch_config: config.batch_config,
        };

        match config.kind {
            WorkerConfigKind::Normal => {
                self.jobs.insert(config.name, factories);
            }
            WorkerConfigKind::Cron {
                schedule,
                queue_key,
                resurrect,
            } => {
                self.jobs.insert(config.name.clone(), factories);

                let schedule = cron::Schedule::from_str(&schedule).unwrap_or_else(|_| {
                    panic!("{}: Invalid cron schedule: {schedule}", config.name)
                });

                self.schedules.insert(
                    config.name,
                    CronJob {
                        schedule,
                        queue_key,
                        resurrect,
                    },
                );
            }
        }
    }

    pub fn worker_names(&self) -> Vec<&str> {
        self.jobs.keys().map(|s| s.as_str()).collect()
    }

    pub fn has_registered(&self, name: &str) -> bool {
        self.jobs.contains_key(name)
    }

    pub fn has_registered_cron(&self, name: &str) -> bool {
        self.schedules.contains_key(name)
    }

    pub(crate) fn batch_config(&self, name: &str) -> Option<WorkerBatchConfig> {
        self.jobs
            .get(name)
            .and_then(|factories| factories.batch_config.clone())
    }

    pub fn build(
        &self,
        name: &str,
        json: serde_json::Value,
        ctx: &DT,
    ) -> Result<BoxedProcessable<ET>, OxanusError> {
        let factory = self
            .jobs
            .get(name)
            .ok_or_else(|| OxanusError::GenericError(format!("Job type {name} not registered")))?;
        match (factory.factory)(json, ctx) {
            Ok(job) => Ok(job),
            Err(e) => Err(OxanusError::JobFactoryError(format!(
                "Failed to build job {name}: {e}"
            ))),
        }
    }

    pub(crate) fn build_batch(
        &self,
        name: &str,
        json: Vec<serde_json::Value>,
        ctx: &DT,
    ) -> Result<BatchBuild<ET>, OxanusError> {
        let factories = self
            .jobs
            .get(name)
            .ok_or_else(|| OxanusError::GenericError(format!("Job type {name} not registered")))?;
        match (factories.batch_factory)(json, ctx) {
            Ok(job) => Ok(job),
            Err(e) => Err(OxanusError::JobFactoryError(format!(
                "Failed to build job batch {name}: {e}"
            ))),
        }
    }
}

impl<DT, ET> Default for WorkerRegistry<DT, ET> {
    fn default() -> Self {
        Self::new()
    }
}
