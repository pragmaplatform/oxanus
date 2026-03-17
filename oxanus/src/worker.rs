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
}

#[async_trait::async_trait]
pub trait Worker<Args: Send + Sync>: Send + Sync {
    type Error: std::error::Error + Send + Sync;

    async fn process(&self, job: &Args, ctx: &JobContext) -> Result<(), Self::Error>;

    fn max_retries(&self) -> u32 {
        2
    }

    fn retry_delay(&self, retries: u32) -> u64 {
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

    fn should_resurrect() -> bool
    where
        Self: Sized,
    {
        true
    }

    fn to_config() -> WorkerConfigKind
    where
        Self: Sized,
    {
        #[allow(clippy::collapsible_if)] // requires 1.88
        if let (Some(schedule), Some(queue_config)) =
            (Self::cron_schedule(), Self::cron_queue_config())
        {
            if let Some(queue_key) = queue_config.static_key() {
                return WorkerConfigKind::Cron {
                    schedule,
                    queue_key,
                    resurrect: Self::should_resurrect(),
                };
            }
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
    pub args: A,
}

#[async_trait::async_trait]
impl<W, A> Processable for BoundJob<W, A>
where
    W: Worker<A> + Send + Sync + 'static,
    A: Send + Sync + 'static,
{
    type Error = W::Error;

    async fn process(&self, ctx: &JobContext) -> Result<(), Self::Error> {
        self.worker.process(&self.args, ctx).await
    }

    fn max_retries(&self) -> u32 {
        self.worker.max_retries()
    }

    fn retry_delay(&self, retries: u32) -> u64 {
        self.worker.retry_delay(retries)
    }
}

#[cfg(feature = "macros")]
#[cfg(test)]
mod tests {
    use super::{Job, JobConflictStrategy};
    use crate as oxanus;
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
        #[derive(Debug, Serialize, Deserialize)]
        struct TestJobArgs {}

        #[derive(oxanus::Worker)]
        #[oxanus(args = TestJobArgs)]
        struct TestWorker;

        impl TestWorker {
            async fn process(
                &self,
                _job: &TestJobArgs,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(oxanus::Worker::<TestJobArgs>::max_retries(&TestWorker), 2);

        #[derive(Debug, Serialize, Deserialize)]
        struct TestJobArgs2 {}

        #[derive(oxanus::Worker)]
        #[oxanus(args = TestJobArgs2)]
        #[oxanus(error = std::fmt::Error, registry = ComponentRegistryFmt)]
        #[oxanus(max_retries = 3, retry_delay = 10)]
        #[oxanus(on_conflict = Replace)]
        struct TestWorkerCustomError;

        impl TestWorkerCustomError {
            async fn process(
                &self,
                _job: &TestJobArgs2,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), std::fmt::Error> {
                use std::fmt::Write;
                let mut s = String::new();
                write!(&mut s, "hi")
            }
        }

        assert_eq!(
            oxanus::Worker::<TestJobArgs2>::max_retries(&TestWorkerCustomError),
            3
        );
        assert_eq!(
            oxanus::Worker::<TestJobArgs2>::retry_delay(&TestWorkerCustomError, 1),
            10
        );
        assert_eq!(TestJobArgs2 {}.on_conflict(), JobConflictStrategy::Replace);

        #[derive(Debug, Serialize, Deserialize)]
        struct UniqueJobArgs {
            id: i32,
            _1: i32,
        }

        #[derive(oxanus::Worker)]
        #[oxanus(args = UniqueJobArgs)]
        #[oxanus(unique_id = "test_worker_{id}")]
        struct TestWorkerUniqueId;

        impl TestWorkerUniqueId {
            async fn process(
                &self,
                _job: &UniqueJobArgs,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(
            oxanus::Worker::<UniqueJobArgs>::max_retries(&TestWorkerUniqueId),
            2
        );
        assert_eq!(
            oxanus::Job::unique_id(&UniqueJobArgs { id: 1, _1: 0 }),
            Some("test_worker_1".to_string())
        );
        assert_eq!(
            oxanus::Job::unique_id(&UniqueJobArgs { id: 12, _1: 0 }),
            Some("test_worker_12".to_string())
        );

        #[derive(Debug, Serialize, Deserialize, Default)]
        struct NestedTask {
            name: String,
        }

        #[derive(Debug, Serialize, Deserialize)]
        struct NestedUniqueJobArgs {
            id: i32,
            task: NestedTask,
        }

        #[derive(oxanus::Worker)]
        #[oxanus(args = NestedUniqueJobArgs)]
        #[oxanus(unique_id(fmt = "test_worker_{id}_{task}", id = self.id, task = self.task.name))]
        struct TestWorkerNestedUniqueId;

        impl TestWorkerNestedUniqueId {
            async fn process(
                &self,
                _job: &NestedUniqueJobArgs,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(
            oxanus::Job::unique_id(&NestedUniqueJobArgs {
                id: 1,
                task: NestedTask {
                    name: "task1".to_owned(),
                }
            }),
            Some("test_worker_1_task1".to_string())
        );
        assert_eq!(
            oxanus::Job::unique_id(&NestedUniqueJobArgs {
                id: 2,
                task: NestedTask {
                    name: "task2".to_owned(),
                }
            }),
            Some("test_worker_2_task2".to_string())
        );

        #[derive(Debug, Serialize, Deserialize)]
        struct CustomUniqueJobArgs {
            id: i32,
            task: NestedTask,
        }

        impl CustomUniqueJobArgs {
            fn unique_id(&self) -> Option<String> {
                Some(format!("worker_id_{}_task_{}", self.id, self.task.name))
            }
        }

        #[derive(oxanus::Worker)]
        #[oxanus(args = CustomUniqueJobArgs)]
        #[oxanus(unique_id = Self::unique_id)]
        #[oxanus(retry_delay = Self::retry_delay)]
        #[oxanus(max_retries = Self::max_retries)]
        struct TestWorkerCustomUniqueId;

        impl TestWorkerCustomUniqueId {
            async fn process(
                &self,
                _job: &CustomUniqueJobArgs,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }

            fn retry_delay(&self, retries: u32) -> u64 {
                retries as u64 * 2
            }

            fn max_retries(&self) -> u32 {
                9
            }
        }

        assert_eq!(
            oxanus::Job::unique_id(&CustomUniqueJobArgs {
                id: 1,
                task: NestedTask {
                    name: "11".to_owned(),
                }
            }),
            Some("worker_id_1_task_11".to_string())
        );
        let job2 = CustomUniqueJobArgs {
            id: 2,
            task: NestedTask {
                name: "22".to_owned(),
            },
        };
        assert_eq!(
            oxanus::Job::unique_id(&job2),
            Some("worker_id_2_task_22".to_string())
        );
        let worker = TestWorkerCustomUniqueId;
        assert_eq!(
            oxanus::Worker::<CustomUniqueJobArgs>::retry_delay(&worker, 1),
            2
        );
        assert_eq!(
            oxanus::Worker::<CustomUniqueJobArgs>::retry_delay(&worker, 2),
            4
        );
        assert_eq!(
            oxanus::Worker::<CustomUniqueJobArgs>::max_retries(&worker),
            9
        );
    }

    #[tokio::test]
    async fn test_define_cron_worker_with_macro() {
        use crate as oxanus;
        use crate::Queue;
        use std::io::Error as WorkerError;

        #[derive(Serialize, oxanus::Queue)]
        struct DefaultQueue;

        #[derive(Debug, Serialize, Deserialize)]
        struct CronArgs {}

        #[derive(oxanus::Worker)]
        #[oxanus(args = CronArgs)]
        #[oxanus(cron(schedule = "*/1 * * * * *", queue = DefaultQueue))]
        struct TestCronWorker;

        impl TestCronWorker {
            async fn process(
                &self,
                _job: &CronArgs,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(
            <TestCronWorker as oxanus::Worker<CronArgs>>::cron_schedule(),
            Some("*/1 * * * * *".to_string())
        );
        assert_eq!(
            <TestCronWorker as oxanus::Worker<CronArgs>>::cron_queue_config(),
            Some(DefaultQueue::to_config()),
        );
        assert!(<TestCronWorker as oxanus::Worker<CronArgs>>::should_resurrect());
    }

    #[tokio::test]
    async fn test_define_worker_with_resurrect_false() {
        use crate as oxanus;
        use std::io::Error as WorkerError;

        #[derive(Debug, Serialize, Deserialize)]
        struct NoResurrectArgs {}

        #[derive(oxanus::Worker)]
        #[oxanus(args = NoResurrectArgs)]
        #[oxanus(resurrect = false)]
        struct NoResurrectWorker;

        impl NoResurrectWorker {
            async fn process(
                &self,
                _job: &NoResurrectArgs,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert!(!<NoResurrectWorker as oxanus::Worker<NoResurrectArgs>>::should_resurrect());

        #[derive(Debug, Serialize, Deserialize)]
        struct DefaultResurrectArgs {}

        #[derive(oxanus::Worker)]
        #[oxanus(args = DefaultResurrectArgs)]
        struct DefaultResurrectWorker;

        impl DefaultResurrectWorker {
            async fn process(
                &self,
                _job: &DefaultResurrectArgs,
                _ctx: &oxanus::JobContext,
            ) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert!(<DefaultResurrectWorker as oxanus::Worker<
            DefaultResurrectArgs,
        >>::should_resurrect());
    }
}
