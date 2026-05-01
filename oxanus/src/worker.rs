use crate::{
    QueueConfig, WorkerConfigKind, context::JobContext, job_envelope::JobConflictStrategy,
};

pub trait Job: Send + Sync + serde::Serialize {
    fn worker_name() -> &'static str
    where
        Self: Sized;

    fn unique_id(&self) -> Option<String> {
        None
    }

    fn on_conflict(&self) -> JobConflictStrategy {
        JobConflictStrategy::Skip
    }

    fn should_resurrect() -> bool
    where
        Self: Sized,
    {
        true
    }

    fn throttle_cost(&self) -> Option<u64> {
        None
    }
}

#[async_trait::async_trait]
pub trait Worker<Args: Send + Sync>: Send + Sync {
    type Error: std::error::Error + Send + Sync;

    async fn process(&self, job: &Args, ctx: &JobContext) -> Result<(), Self::Error>;

    fn max_retries(&self, _job: &Args) -> u32 {
        2
    }

    fn retry_delay(&self, _job: &Args, retries: u32) -> u64 {
        // 0 -> 25 seconds
        // 1 -> 125 seconds
        // 2 -> 625 seconds
        // 3 -> 3125 seconds
        // 4 -> 15625 seconds
        // 5 -> 78125 seconds
        // 6 -> 390625 seconds
        // 7 -> 1953125 seconds
        u64::pow(5, retries + 2)
    }

    /// 6 part cron schedule: "* * * * * *"
    fn cron_schedule() -> Option<String>
    where
        Self: Sized,
    {
        None
    }

    fn cron_queue_config() -> Option<QueueConfig>
    where
        Self: Sized,
    {
        None
    }

    fn to_config() -> WorkerConfigKind
    where
        Self: Sized,
        Args: Job,
    {
        if let Some(schedule) = Self::cron_schedule() {
            let queue_config = Self::cron_queue_config()
                .expect("Cron worker must define cron_queue_config (use #[oxanus(cron(schedule = \"...\", queue = MyQueue))])");
            let queue_key = queue_config.static_key().expect(
                "Cron workers must use static queues. Dynamic queues are not supported for cron workers.",
            );
            return WorkerConfigKind::Cron {
                schedule,
                queue_key,
                resurrect: Args::should_resurrect(),
            };
        }
        WorkerConfigKind::Normal
    }
}

pub trait FromContext<T> {
    fn from_context(ctx: &T) -> Self;
}

#[async_trait::async_trait]
pub trait Processable: Send + Sync {
    type Error: std::error::Error + Send + Sync;

    async fn process(&self, ctx: &JobContext) -> Result<(), Self::Error>;
    fn max_retries(&self) -> u32;
    fn retry_delay(&self, retries: u32) -> u64;
}

pub type BoxedProcessable<ET> = Box<dyn Processable<Error = ET>>;

pub(crate) struct BoundJob<W, A> {
    pub worker: W,
    pub job: A,
}

#[async_trait::async_trait]
impl<W, A> Processable for BoundJob<W, A>
where
    W: Worker<A> + Send + Sync + 'static,
    A: Send + Sync + 'static,
{
    type Error = W::Error;

    async fn process(&self, ctx: &JobContext) -> Result<(), Self::Error> {
        self.worker.process(&self.job, ctx).await
    }

    fn max_retries(&self) -> u32 {
        self.worker.max_retries(&self.job)
    }

    fn retry_delay(&self, retries: u32) -> u64 {
        self.worker.retry_delay(&self.job, retries)
    }
}

#[cfg(feature = "macros")]
#[cfg(test)]
mod tests {
    use super::{Job, JobConflictStrategy};
    use crate::{self as oxanus, JobEnvelope};
    use serde::{Deserialize, Serialize};
    use std::io::Error as WorkerError;

    #[derive(Clone, Default)]
    struct WorkerContext {}

    #[derive(oxanus::Registry)]
    #[allow(dead_code)]
    struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

    #[derive(oxanus::Registry)]
    #[allow(dead_code)]
    struct ComponentRegistryFmt(oxanus::ComponentRegistry<WorkerContext, std::fmt::Error>);

    #[tokio::test]
    async fn test_define_worker_with_macro() {
        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        struct TestJob {}

        #[derive(oxanus::Worker)]
        struct TestWorker;

        impl TestWorker {
            async fn process(
                &self,
                _job: &TestJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(
            oxanus::Worker::<TestJob>::max_retries(&TestWorker, &TestJob {}),
            2
        );
        assert_eq!(TestJob::worker_name(), std::any::type_name::<TestWorker>());

        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        #[oxanus(worker = TestWorkerCustomError, on_conflict = Replace)]
        struct TestWorkerCustomErrorJob {}

        #[derive(oxanus::Worker)]
        #[oxanus(error = std::fmt::Error, registry = ComponentRegistryFmt)]
        #[oxanus(max_retries = 3, retry_delay = 10)]
        struct TestWorkerCustomError;

        impl TestWorkerCustomError {
            async fn process(
                &self,
                _job: &TestWorkerCustomErrorJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), std::fmt::Error> {
                use std::fmt::Write;
                let mut s = String::new();
                write!(&mut s, "hi")
            }
        }

        assert_eq!(
            oxanus::Worker::<TestWorkerCustomErrorJob>::max_retries(
                &TestWorkerCustomError,
                &TestWorkerCustomErrorJob {}
            ),
            3
        );
        assert_eq!(
            oxanus::Worker::<TestWorkerCustomErrorJob>::retry_delay(
                &TestWorkerCustomError,
                &TestWorkerCustomErrorJob {},
                1
            ),
            10
        );
        assert_eq!(
            TestWorkerCustomErrorJob {}.on_conflict(),
            JobConflictStrategy::Replace
        );

        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        #[oxanus(worker = TestWorkerUniqueId)]
        #[oxanus(unique_id = "test_worker_{id}")]
        struct TestWorkerUniqueIdJob {
            id: i32,
            _1: i32,
        }

        #[derive(oxanus::Worker)]
        struct TestWorkerUniqueId;

        impl TestWorkerUniqueId {
            async fn process(
                &self,
                _job: &TestWorkerUniqueIdJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(
            oxanus::Worker::<TestWorkerUniqueIdJob>::max_retries(
                &TestWorkerUniqueId,
                &TestWorkerUniqueIdJob { id: 0, _1: 0 }
            ),
            2
        );
        assert_eq!(
            oxanus::Job::unique_id(&TestWorkerUniqueIdJob { id: 1, _1: 0 }),
            Some("test_worker_1".to_string())
        );
        assert_eq!(
            oxanus::Job::unique_id(&TestWorkerUniqueIdJob { id: 12, _1: 0 }),
            Some("test_worker_12".to_string())
        );

        #[derive(Debug, Serialize, Deserialize, Default)]
        struct NestedTask {
            name: String,
        }

        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        #[oxanus(worker = TestWorkerNestedUniqueId)]
        #[oxanus(unique_id(fmt = "test_worker_{id}_{task}", id = self.id, task = self.task.name))]
        struct TestWorkerNestedUniqueIdJob {
            id: i32,
            task: NestedTask,
        }

        #[derive(oxanus::Worker)]
        struct TestWorkerNestedUniqueId;

        impl TestWorkerNestedUniqueId {
            async fn process(
                &self,
                _job: &TestWorkerNestedUniqueIdJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(
            oxanus::Job::unique_id(&TestWorkerNestedUniqueIdJob {
                id: 1,
                task: NestedTask {
                    name: "task1".to_owned(),
                }
            }),
            Some("test_worker_1_task1".to_string())
        );
        assert_eq!(
            oxanus::Job::unique_id(&TestWorkerNestedUniqueIdJob {
                id: 2,
                task: NestedTask {
                    name: "task2".to_owned(),
                }
            }),
            Some("test_worker_2_task2".to_string())
        );

        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        #[oxanus(worker = TestWorkerCustomUniqueId)]
        #[oxanus(unique_id = Self::unique_id)]
        #[oxanus(throttle_cost = Self::throttle_cost)]
        struct TestWorkerCustomUniqueIdJob {
            id: i32,
            task: NestedTask,
            cost: u64,
        }

        impl TestWorkerCustomUniqueIdJob {
            fn unique_id(&self) -> Option<String> {
                Some(format!("worker_id_{}_task_{}", self.id, self.task.name))
            }

            fn throttle_cost(&self) -> Option<u64> {
                Some(self.cost)
            }
        }

        #[derive(oxanus::Worker)]
        #[oxanus(retry_delay = Self::retry_delay)]
        #[oxanus(max_retries = Self::max_retries)]
        struct TestWorkerCustomUniqueId;

        impl TestWorkerCustomUniqueId {
            async fn process(
                &self,
                _job: &TestWorkerCustomUniqueIdJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }

            fn retry_delay(&self, _job: &TestWorkerCustomUniqueIdJob, retries: u32) -> u64 {
                retries as u64 * 2
            }

            fn max_retries(&self, _job: &TestWorkerCustomUniqueIdJob) -> u32 {
                9
            }
        }

        assert_eq!(
            oxanus::Job::unique_id(&TestWorkerCustomUniqueIdJob {
                id: 1,
                task: NestedTask {
                    name: "11".to_owned(),
                },
                cost: 3,
            }),
            Some("worker_id_1_task_11".to_string())
        );
        let job2 = TestWorkerCustomUniqueIdJob {
            id: 2,
            task: NestedTask {
                name: "22".to_owned(),
            },
            cost: 5,
        };
        assert_eq!(
            oxanus::Job::unique_id(&job2),
            Some("worker_id_2_task_22".to_string())
        );
        assert_eq!(oxanus::Job::throttle_cost(&job2), Some(5));
        let worker = TestWorkerCustomUniqueId;
        assert_eq!(
            oxanus::Worker::<TestWorkerCustomUniqueIdJob>::retry_delay(&worker, &job2, 1),
            2
        );
        assert_eq!(
            oxanus::Worker::<TestWorkerCustomUniqueIdJob>::retry_delay(&worker, &job2, 2),
            4
        );
        assert_eq!(
            oxanus::Worker::<TestWorkerCustomUniqueIdJob>::max_retries(&worker, &job2),
            9
        );
        let envelope = JobEnvelope::new(
            "default".to_owned(),
            TestWorkerCustomUniqueIdJob {
                id: 3,
                task: NestedTask {
                    name: "33".to_owned(),
                },
                cost: 7,
            },
        )
        .expect("job-owned throttle_cost should populate the envelope");
        assert_eq!(envelope.meta.throttle_cost, Some(7));

        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        #[oxanus(worker = TestWorkerExplicitJobHooks)]
        #[oxanus(unique_id = TestWorkerExplicitJobHooksJob::unique_id)]
        #[oxanus(throttle_cost = TestWorkerExplicitJobHooksJob::throttle_cost)]
        struct TestWorkerExplicitJobHooksJob {
            id: i32,
            cost: u64,
        }

        impl TestWorkerExplicitJobHooksJob {
            fn unique_id(&self) -> Option<String> {
                Some(format!("explicit_job_{}", self.id))
            }

            fn throttle_cost(&self) -> Option<u64> {
                Some(self.cost)
            }
        }

        #[derive(oxanus::Worker)]
        struct TestWorkerExplicitJobHooks;

        impl TestWorkerExplicitJobHooks {
            async fn process(
                &self,
                _job: &TestWorkerExplicitJobHooksJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        let explicit_job = TestWorkerExplicitJobHooksJob { id: 4, cost: 9 };
        assert_eq!(
            oxanus::Job::unique_id(&explicit_job),
            Some("explicit_job_4".to_string())
        );
        assert_eq!(oxanus::Job::throttle_cost(&explicit_job), Some(9));
        let explicit_envelope = JobEnvelope::new(
            "default".to_owned(),
            TestWorkerExplicitJobHooksJob { id: 5, cost: 11 },
        )
        .expect("explicit job hook paths should still populate the envelope");
        assert_eq!(explicit_envelope.meta.throttle_cost, Some(11));
    }

    #[tokio::test]
    async fn test_define_cron_worker_with_macro() {
        use crate as oxanus;
        use crate::Queue;
        use std::io::Error as WorkerError;

        #[derive(Serialize, oxanus::Queue)]
        struct DefaultQueue;

        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        struct TestCronJob {}

        #[derive(oxanus::Worker)]
        #[oxanus(cron(schedule = "*/1 * * * * *", queue = DefaultQueue))]
        struct TestCronWorker;

        impl TestCronWorker {
            async fn process(
                &self,
                _job: &TestCronJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(
            <TestCronWorker as oxanus::Worker<TestCronJob>>::cron_schedule(),
            Some("*/1 * * * * *".to_string())
        );
        assert_eq!(
            <TestCronWorker as oxanus::Worker<TestCronJob>>::cron_queue_config(),
            Some(DefaultQueue::to_config()),
        );
        assert!(<TestCronJob as oxanus::Job>::should_resurrect());
    }

    #[tokio::test]
    async fn test_define_worker_with_resurrect_false() {
        use crate as oxanus;
        use std::io::Error as WorkerError;

        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        #[oxanus(resurrect = false)]
        struct NoResurrectJob {}

        #[derive(oxanus::Worker)]
        struct NoResurrectWorker;

        impl NoResurrectWorker {
            async fn process(
                &self,
                _job: &NoResurrectJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert!(!<NoResurrectJob as oxanus::Job>::should_resurrect());

        #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
        struct DefaultResurrectJob {}

        #[derive(oxanus::Worker)]
        struct DefaultResurrectWorker;

        impl DefaultResurrectWorker {
            async fn process(
                &self,
                _job: &DefaultResurrectJob,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert!(<DefaultResurrectJob as oxanus::Job>::should_resurrect());
    }
}
