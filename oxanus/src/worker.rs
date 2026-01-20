use crate::{QueueConfig, WorkerConfigKind, context::Context, job_envelope::JobConflictStrategy};
use std::panic::UnwindSafe;

pub type BoxedWorker<DT, ET> = Box<dyn Worker<Context = DT, Error = ET>>;

#[async_trait::async_trait]
pub trait Worker: Send + Sync + UnwindSafe {
    type Context: Clone + Send + Sync;
    type Error: std::error::Error + Send + Sync;

    async fn process(&self, data: &Context<Self::Context>) -> Result<(), Self::Error>;

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

    fn unique_id(&self) -> Option<String> {
        None
    }

    fn on_conflict(&self) -> JobConflictStrategy {
        JobConflictStrategy::Skip
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
    {
        #[allow(clippy::collapsible_if)] // requires 1.88
        if let (Some(schedule), Some(queue_config)) =
            (Self::cron_schedule(), Self::cron_queue_config())
        {
            if let Some(queue_key) = queue_config.static_key() {
                return WorkerConfigKind::Cron {
                    schedule,
                    queue_key,
                };
            }
        }
        WorkerConfigKind::Normal
    }
}

#[cfg(feature = "macros")]
#[cfg(test)]
mod tests {
    use super::{JobConflictStrategy, Worker};
    use crate as oxanus;
    use crate::Context;
    use crate::test_helper::create_worker_context;
    use serde::{Deserialize, Serialize};
    use std::io::Error as WorkerError;
    use std::sync::{Arc, Mutex}; // needed for unit test

    #[derive(Clone, Default)]
    struct WorkerContext {
        count: Arc<Mutex<usize>>,
    }

    #[derive(oxanus::Registry)]
    #[allow(dead_code)]
    struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

    #[derive(oxanus::Registry)]
    #[allow(dead_code)]
    struct ComponentRegistryFmt(oxanus::ComponentRegistry<WorkerContext, std::fmt::Error>);

    #[tokio::test]
    async fn test_define_worker_with_macro() {
        #[derive(Serialize, Deserialize, oxanus::Worker)]
        struct TestWorker {}

        impl TestWorker {
            async fn process(&self, ctx: &Context<WorkerContext>) -> Result<(), WorkerError> {
                *ctx.ctx
                    .count
                    .lock()
                    .map_err(|e| std::io::Error::other(e.to_string()))? += 1;
                Ok(())
            }
        }

        assert_eq!(TestWorker {}.max_retries(), 2);
        assert_eq!(TestWorker {}.on_conflict(), JobConflictStrategy::Skip);

        let ctx = WorkerContext::default();
        let context = create_worker_context(ctx.clone(), TestWorker {}).await;

        Worker::process(&TestWorker {}, &context).await.unwrap();

        assert_eq!(*ctx.count.lock().unwrap(), 1);

        Worker::process(&TestWorker {}, &context).await.unwrap();

        assert_eq!(*ctx.count.lock().unwrap(), 2);

        #[derive(Serialize, Deserialize, oxanus::Worker)]
        #[oxanus(error = std::fmt::Error, registry = ComponentRegistryFmt)]
        #[oxanus(max_retries = 3, retry_delay = 10)]
        #[oxanus(on_conflict = Replace)]
        struct TestWorkerCustomError {}

        impl TestWorkerCustomError {
            async fn process(&self, _: &Context<WorkerContext>) -> Result<(), std::fmt::Error> {
                use std::fmt::Write;

                let mut s = String::new();
                write!(&mut s, "hi")
            }
        }

        assert_eq!(TestWorkerCustomError {}.unique_id(), None);
        assert_eq!(TestWorkerCustomError {}.max_retries(), 3);
        assert_eq!(TestWorkerCustomError {}.retry_delay(1), 10);
        assert_eq!(
            TestWorkerCustomError {}.on_conflict(),
            JobConflictStrategy::Replace
        );

        #[derive(Serialize, Deserialize, oxanus::Worker)]
        #[oxanus(unique_id = "test_worker_{id}")]
        struct TestWorkerUniqueId {
            id: i32,
            _1: i32,
        }

        impl TestWorkerUniqueId {
            async fn process(&self, _: &Context<WorkerContext>) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(TestWorkerUniqueId { id: 1, _1: 0 }.max_retries(), 2);
        assert_eq!(
            TestWorkerUniqueId { id: 1, _1: 0 }.unique_id().unwrap(),
            "test_worker_1"
        );
        assert_eq!(
            TestWorkerUniqueId { id: 12, _1: 0 }.unique_id().unwrap(),
            "test_worker_12"
        );

        #[derive(Serialize, Deserialize, oxanus::Worker)]
        #[oxanus(unique_id(fmt = "test_worker_{id}_{task}", id = self.id, task = self.task.name))]
        struct TestWorkerNestedUniqueId {
            id: i32,
            task: TestWorkerNestedTask,
        }

        #[derive(Serialize, Deserialize, Default)]
        struct TestWorkerNestedTask {
            name: String,
        }

        impl TestWorkerNestedUniqueId {
            async fn process(&self, _: &Context<WorkerContext>) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(
            TestWorkerNestedUniqueId {
                id: 1,
                task: Default::default()
            }
            .max_retries(),
            2
        );
        assert_eq!(
            TestWorkerNestedUniqueId {
                id: 1,
                task: TestWorkerNestedTask {
                    name: "task1".to_owned(),
                }
            }
            .unique_id()
            .unwrap(),
            "test_worker_1_task1"
        );
        assert_eq!(
            TestWorkerNestedUniqueId {
                id: 2,
                task: TestWorkerNestedTask {
                    name: "task2".to_owned(),
                }
            }
            .unique_id()
            .unwrap(),
            "test_worker_2_task2"
        );

        #[derive(Serialize, Deserialize, oxanus::Worker)]
        #[oxanus(unique_id = Self::unique_id)]
        #[oxanus(retry_delay = Self::retry_delay)]
        #[oxanus(max_retries = Self::max_retries)]
        struct TestWorkerCustomUniqueId {
            id: i32,
            task: TestWorkerNestedTask,
        }

        impl TestWorkerCustomUniqueId {
            async fn process(&self, _: &Context<WorkerContext>) -> Result<(), WorkerError> {
                Ok(())
            }

            fn unique_id(&self) -> Option<String> {
                Some(format!("worker_id_{}_task_{}", self.id, self.task.name))
            }

            fn retry_delay(&self, retries: u32) -> u64 {
                retries as u64 * 2
            }

            fn max_retries(&self) -> u32 {
                9
            }
        }

        assert_eq!(
            Worker::unique_id(&TestWorkerCustomUniqueId {
                id: 1,
                task: TestWorkerNestedTask {
                    name: "11".to_owned(),
                }
            })
            .unwrap(),
            "worker_id_1_task_11"
        );
        let worker2 = TestWorkerCustomUniqueId {
            id: 2,
            task: TestWorkerNestedTask {
                name: "22".to_owned(),
            },
        };
        assert_eq!(Worker::unique_id(&worker2).unwrap(), "worker_id_2_task_22");
        assert_eq!(worker2.retry_delay(1), 2);
        assert_eq!(worker2.retry_delay(2), 4);
        assert_eq!(worker2.max_retries(), 9);
    }

    #[tokio::test]
    async fn test_define_cron_worker_with_macro() {
        use crate as oxanus; // needed for unit test
        use crate::Queue;
        use std::io::Error as WorkerError;

        #[derive(Serialize, oxanus::Queue)]
        struct DefaultQueue;

        #[derive(Serialize, Deserialize, oxanus::Worker)]
        #[oxanus(cron(schedule = "*/1 * * * * *", queue = DefaultQueue))]
        struct TestCronWorker {}

        impl TestCronWorker {
            async fn process(&self, _: &Context<WorkerContext>) -> Result<(), WorkerError> {
                Ok(())
            }
        }

        assert_eq!(TestCronWorker {}.unique_id(), None);
        assert_eq!(TestCronWorker::cron_schedule().unwrap(), "*/1 * * * * *");
        assert_eq!(
            TestCronWorker::cron_queue_config().unwrap(),
            DefaultQueue::to_config(),
        );
    }
}
