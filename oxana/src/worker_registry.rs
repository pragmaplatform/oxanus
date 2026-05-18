use std::collections::HashMap;
use std::str::FromStr;

use crate::WorkerBatchConfig;
use crate::error::OxanaError;
use crate::job_envelope::JobEnvelope;
use crate::worker::{BoundBatchJob, BoundJob, BoxedProcessable, FromContext, Job, Worker};

pub type JobFactory<DT> = fn(serde_json::Value, &DT) -> Result<BoxedProcessable, OxanaError>;
pub type JobBatchFactory<DT> = fn(Vec<serde_json::Value>, &DT) -> Result<BatchBuild, OxanaError>;
pub type JobEnvelopeFactory = fn(String, serde_json::Value) -> Result<JobEnvelope, OxanaError>;

#[derive(Clone)]
pub struct OnDemandJobRegistration {
    pub args_template: serde_json::Value,
    pub enqueue_factory: JobEnvelopeFactory,
}

pub struct BatchBuild {
    pub job: Option<BoxedProcessable>,
    pub invalid: Vec<InvalidBatchJob>,
}

pub struct InvalidBatchJob {
    pub index: usize,
    pub error: String,
}

#[derive(Clone)]
pub struct WorkerRegistry<DT> {
    jobs: HashMap<String, WorkerFactories<DT>>,
    aliases: HashMap<String, String>,
    pub schedules: HashMap<String, CronJob>,
    pub on_demand_jobs: HashMap<String, OnDemandJobRegistration>,
}

pub struct WorkerConfig<DT> {
    pub name: String,
    pub legacy_names: Vec<String>,
    pub factory: JobFactory<DT>,
    pub batch_factory: JobBatchFactory<DT>,
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

pub fn job_factory<W, A, DT>(
    value: serde_json::Value,
    ctx: &DT,
) -> Result<BoxedProcessable, OxanaError>
where
    W: Worker<A> + FromContext<DT> + 'static,
    A: Job + serde::de::DeserializeOwned + Send + 'static,
    DT: Send + Sync + Clone + 'static,
{
    let job: A = serde_json::from_value(value)?;
    let worker = W::from_context(ctx);
    Ok(Box::new(BoundJob { worker, job }))
}

pub fn job_batch_factory<W, A, DT>(
    values: Vec<serde_json::Value>,
    ctx: &DT,
) -> Result<BatchBuild, OxanaError>
where
    W: Worker<A> + FromContext<DT> + 'static,
    A: Job + serde::de::DeserializeOwned + Send + 'static,
    DT: Send + Sync + Clone + 'static,
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

#[derive(Clone)]
struct WorkerFactories<DT> {
    factory: JobFactory<DT>,
    batch_factory: JobBatchFactory<DT>,
    batch_config: Option<WorkerBatchConfig>,
}

impl<DT> WorkerRegistry<DT> {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            aliases: HashMap::new(),
            schedules: HashMap::new(),
            on_demand_jobs: HashMap::new(),
        }
    }

    pub fn register_worker_with(&mut self, config: WorkerConfig<DT>) {
        let name = config.name;
        let legacy_names = config.legacy_names;
        let factories = WorkerFactories {
            factory: config.factory,
            batch_factory: config.batch_factory,
            batch_config: config.batch_config,
        };

        if let Some(alias_target) = self.aliases.remove(&name) {
            tracing::warn!(
                alias = name,
                target = alias_target,
                "Removing legacy worker alias because it collides with a registered job name"
            );
        }

        if let Some(on_demand) = config.on_demand {
            self.on_demand_jobs.insert(name.clone(), on_demand);
        } else {
            self.on_demand_jobs.remove(&name);
        }

        match config.kind {
            WorkerConfigKind::Normal => {
                self.jobs.insert(name.clone(), factories);
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
                    name.clone(),
                    CronJob {
                        schedule,
                        queue_key,
                        resurrect,
                    },
                );
            }
        }

        for legacy_name in legacy_names {
            self.register_legacy_name(&name, legacy_name);
        }
    }

    pub fn worker_names(&self) -> Vec<&str> {
        self.jobs.keys().map(|s| s.as_str()).collect()
    }

    pub(crate) fn batch_config(&self, name: &str) -> Option<WorkerBatchConfig> {
        self.factories_for(name)
            .and_then(|factories| factories.batch_config.clone())
    }

    pub fn build(
        &self,
        name: &str,
        json: serde_json::Value,
        ctx: &DT,
    ) -> Result<BoxedProcessable, OxanaError> {
        let factory = self
            .factories_for(name)
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
    ) -> Result<BatchBuild, OxanaError> {
        let factories = self
            .factories_for(name)
            .ok_or_else(|| OxanaError::GenericError(format!("Job type {name} not registered")))?;
        match (factories.batch_factory)(json, ctx) {
            Ok(job) => Ok(job),
            Err(e) => Err(OxanaError::JobFactoryError(format!(
                "Failed to build job batch {name}: {e}"
            ))),
        }
    }

    fn register_legacy_name(&mut self, canonical_name: &str, legacy_name: String) {
        if legacy_name == canonical_name {
            return;
        }

        if self.jobs.contains_key(&legacy_name) {
            tracing::warn!(
                alias = legacy_name,
                target = canonical_name,
                "Skipping legacy worker alias because it collides with a registered job name"
            );
            return;
        }

        match self.aliases.get(&legacy_name) {
            Some(existing_target) if existing_target == canonical_name => {}
            Some(existing_target) => {
                tracing::warn!(
                    alias = legacy_name,
                    target = canonical_name,
                    existing_target = existing_target,
                    "Skipping legacy worker alias because it already points to another job"
                );
            }
            None => {
                self.aliases.insert(legacy_name, canonical_name.to_owned());
            }
        }
    }

    fn factories_for(&self, name: &str) -> Option<&WorkerFactories<DT>> {
        let canonical_name = self.resolve_name(name);
        self.jobs.get(canonical_name)
    }

    fn resolve_name<'a>(&'a self, name: &'a str) -> &'a str {
        if self.jobs.contains_key(name) {
            name
        } else {
            self.aliases.get(name).map_or(name, String::as_str)
        }
    }
}

impl<DT> Default for WorkerRegistry<DT> {
    fn default() -> Self {
        Self::new()
    }
}
