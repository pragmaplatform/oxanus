use std::collections::HashMap;
use std::str::FromStr;

use crate::WorkerBatchConfig;
use crate::error::OxanaError;
use crate::job_envelope::JobEnvelope;
use crate::worker::{BoundBatchJob, BoundJob, BoxedProcessable, FromContext, Job, Worker};

pub type JobFactory<DT, ET> =
    fn(serde_json::Value, &DT) -> Result<BoxedProcessable<ET>, OxanaError>;
pub type JobBatchFactory<DT, ET> =
    fn(Vec<serde_json::Value>, &DT) -> Result<BatchBuild<ET>, OxanaError>;
pub type JobEnvelopeFactory = fn(String, serde_json::Value) -> Result<JobEnvelope, OxanaError>;

#[derive(Clone)]
pub struct OnDemandJobRegistration {
    pub args_template: serde_json::Value,
    pub enqueue_factory: JobEnvelopeFactory,
}

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
    pub on_demand_jobs: HashMap<String, OnDemandJobRegistration>,
}

pub struct WorkerConfig<DT, ET> {
    pub name: String,
    pub factory: JobFactory<DT, ET>,
    pub batch_factory: JobBatchFactory<DT, ET>,
    pub batch_config: Option<WorkerBatchConfig>,
    pub on_demand: Option<OnDemandJobRegistration>,
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
) -> Result<BoxedProcessable<ET>, OxanaError>
where
    W: Worker<A, Error = ET> + FromContext<DT> + 'static,
    A: Job + serde::de::DeserializeOwned + Send + 'static,
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
) -> Result<BatchBuild<ET>, OxanaError>
where
    W: Worker<A, Error = ET> + FromContext<DT> + 'static,
    A: Job + serde::de::DeserializeOwned + Send + 'static,
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

pub fn job_envelope_factory<A>(
    queue: String,
    value: serde_json::Value,
) -> Result<JobEnvelope, OxanaError>
where
    A: Job + serde::de::DeserializeOwned + Send + 'static,
{
    let job: A = serde_json::from_value(value)?;
    JobEnvelope::new(queue, job)
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
            on_demand_jobs: HashMap::new(),
        }
    }

    pub fn register_worker_with(&mut self, config: WorkerConfig<DT, ET>) {
        let name = config.name;
        let factories = WorkerFactories {
            factory: config.factory,
            batch_factory: config.batch_factory,
            batch_config: config.batch_config,
        };

        if let Some(on_demand) = config.on_demand {
            self.on_demand_jobs.insert(name.clone(), on_demand);
        } else {
            self.on_demand_jobs.remove(&name);
        }

        match config.kind {
            WorkerConfigKind::Normal => {
                self.jobs.insert(name, factories);
            }
            WorkerConfigKind::Cron {
                schedule,
                queue_key,
                resurrect,
            } => {
                self.jobs.insert(name.clone(), factories);

                let schedule = cron::Schedule::from_str(&schedule)
                    .unwrap_or_else(|_| panic!("{}: Invalid cron schedule: {schedule}", name));

                self.schedules.insert(
                    name,
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
    ) -> Result<BoxedProcessable<ET>, OxanaError> {
        let factory = self
            .jobs
            .get(name)
            .ok_or_else(|| OxanaError::GenericError(format!("Job type {name} not registered")))?;
        match (factory.factory)(json, ctx) {
            Ok(job) => Ok(job),
            Err(e) => Err(OxanaError::JobFactoryError(format!(
                "Failed to build job {name}: {e}"
            ))),
        }
    }

    pub(crate) fn build_batch(
        &self,
        name: &str,
        json: Vec<serde_json::Value>,
        ctx: &DT,
    ) -> Result<BatchBuild<ET>, OxanaError> {
        let factories = self
            .jobs
            .get(name)
            .ok_or_else(|| OxanaError::GenericError(format!("Job type {name} not registered")))?;
        match (factories.batch_factory)(json, ctx) {
            Ok(job) => Ok(job),
            Err(e) => Err(OxanaError::JobFactoryError(format!(
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
