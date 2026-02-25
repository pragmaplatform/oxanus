use std::str::FromStr;
use std::{any::type_name, collections::HashMap};

use crate::error::OxanusError;
use crate::worker::Worker;

type BoxedJob<DT, ET> = Box<dyn Worker<Context = DT, Error = ET>>;
type JobFactory<DT, ET> = fn(serde_json::Value) -> Result<BoxedJob<DT, ET>, OxanusError>;

pub struct WorkerRegistry<DT, ET> {
    jobs: HashMap<String, JobFactory<DT, ET>>,
    pub schedules: HashMap<String, CronJob>,
}

pub struct WorkerConfig<DT, ET> {
    pub name: String,
    pub factory: JobFactory<DT, ET>,
    pub kind: WorkerConfigKind,
}

pub enum WorkerConfigKind {
    Normal,
    Cron { schedule: String, queue_key: String },
}

#[derive(Debug, Clone)]
pub struct CronJob {
    pub schedule: cron::Schedule,
    pub queue_key: String,
}

pub fn job_factory<
    T: Worker<Context = DT, Error = ET> + serde::de::DeserializeOwned + 'static,
    DT,
    ET,
>(
    value: serde_json::Value,
) -> Result<BoxedJob<DT, ET>, OxanusError> {
    let job: T = serde_json::from_value(value)?;
    Ok(Box::new(job))
}

impl<DT, ET> WorkerRegistry<DT, ET> {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            schedules: HashMap::new(),
        }
    }

    pub fn register<T>(&mut self) -> &mut Self
    where
        T: Worker<Context = DT, Error = ET> + serde::de::DeserializeOwned + 'static,
    {
        let name = type_name::<T>();

        self.jobs.insert(name.to_string(), job_factory::<T, DT, ET>);
        self
    }

    pub fn register_cron<T>(&mut self, schedule: &str, queue_key: String) -> &mut Self
    where
        T: Worker<Context = DT, Error = ET> + serde::de::DeserializeOwned + 'static,
    {
        self.validate_cron_worker::<T>();

        let name = type_name::<T>();
        let schedule = cron::Schedule::from_str(schedule)
            .unwrap_or_else(|_| panic!("{name}: Invalid cron schedule: {schedule}"));

        self.register::<T>();
        self.schedules.insert(
            name.to_string(),
            CronJob {
                schedule,
                queue_key,
            },
        );

        self
    }

    pub fn register_worker_with(&mut self, config: WorkerConfig<DT, ET>) {
        match config.kind {
            WorkerConfigKind::Normal => {
                self.jobs.insert(config.name, config.factory);
            }
            WorkerConfigKind::Cron {
                schedule,
                queue_key,
            } => {
                // we can enforce cron worker being `struct Worker {}` in macro

                self.jobs.insert(config.name.clone(), config.factory);

                let schedule = cron::Schedule::from_str(&schedule).unwrap_or_else(|_| {
                    panic!("{}: Invalid cron schedule: {schedule}", config.name)
                });

                self.schedules.insert(
                    config.name,
                    CronJob {
                        schedule,
                        queue_key,
                    },
                );
            }
        }
    }

    pub fn worker_names(&self) -> Vec<&str> {
        self.jobs.keys().map(|s| s.as_str()).collect()
    }

    pub fn has_registered<T>(&self) -> bool
    where
        T: Worker<Context = DT, Error = ET>,
    {
        self.jobs.contains_key(type_name::<T>())
    }

    pub fn has_registered_cron<T>(&self) -> bool
    where
        T: Worker<Context = DT, Error = ET>,
    {
        self.schedules.contains_key(type_name::<T>())
    }

    pub fn build(
        &self,
        name: &str,
        json: serde_json::Value,
    ) -> Result<BoxedJob<DT, ET>, OxanusError> {
        let factory = self
            .jobs
            .get(name)
            .ok_or_else(|| OxanusError::GenericError(format!("Job type {name} not registered")))?;
        match factory(json) {
            Ok(job) => Ok(job),
            Err(e) => Err(OxanusError::JobFactoryError(format!(
                "Failed to build job {name}: {e}"
            ))),
        }
    }

    fn validate_cron_worker<T>(&self)
    where
        T: Worker<Context = DT, Error = ET> + serde::de::DeserializeOwned + 'static,
    {
        let name = type_name::<T>();
        if serde_json::from_value::<T>(serde_json::json!({})).is_err() {
            panic!(
                "{name}: Cron worker must be deserializable from empty JSON (Don't define worker as `struct {name};`, use `struct {name} {{}}` instead)"
            );
        }
    }
}

impl<DT, ET> Default for WorkerRegistry<DT, ET> {
    fn default() -> Self {
        Self::new()
    }
}
