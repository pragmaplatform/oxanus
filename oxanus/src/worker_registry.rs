use std::collections::HashMap;
use std::str::FromStr;

use crate::batch_processor::BatchProcessorConfig;
use crate::error::OxanusError;
use crate::worker::{BoundJob, BoxedProcessable, FromContext, Worker};

pub(crate) type JobFactory<DT, ET> =
    fn(serde_json::Value, &DT) -> Result<BoxedProcessable<ET>, OxanusError>;

pub struct WorkerRegistry<DT, ET> {
    jobs: HashMap<String, JobFactory<DT, ET>>,
    pub schedules: HashMap<String, CronJob>,
    pub batch_configs: HashMap<String, BatchProcessorConfig<DT, ET>>,
}

pub struct WorkerConfig<DT, ET> {
    pub name: String,
    pub factory: JobFactory<DT, ET>,
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
    A: serde::de::DeserializeOwned + Send + Sync + 'static,
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let args: A = serde_json::from_value(value)?;
    let worker = W::from_context(ctx);
    Ok(Box::new(BoundJob { worker, args }))
}

impl<DT, ET> WorkerRegistry<DT, ET> {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            schedules: HashMap::new(),
            batch_configs: HashMap::new(),
        }
    }

    pub fn register_worker_with(&mut self, config: WorkerConfig<DT, ET>) {
        match config.kind {
            WorkerConfigKind::Normal => {
                self.jobs.insert(config.name, config.factory);
            }
            WorkerConfigKind::Cron {
                schedule,
                queue_key,
                resurrect,
            } => {
                self.jobs.insert(config.name.clone(), config.factory);

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

    pub fn register_batch_processor_with(&mut self, config: BatchProcessorConfig<DT, ET>) {
        self.batch_configs
            .insert(config.worker_name.clone(), config);
    }

    pub fn get_batch_config(&self, worker_name: &str) -> Option<&BatchProcessorConfig<DT, ET>> {
        self.batch_configs.get(worker_name)
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
        match factory(json, ctx) {
            Ok(job) => Ok(job),
            Err(e) => Err(OxanusError::JobFactoryError(format!(
                "Failed to build job {name}: {e}"
            ))),
        }
    }
}

impl<DT, ET> Default for WorkerRegistry<DT, ET> {
    fn default() -> Self {
        Self::new()
    }
}
