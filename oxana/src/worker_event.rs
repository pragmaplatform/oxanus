use crate::semaphores_map::QueuePermit;

#[derive(Debug)]
pub struct WorkerJob {
    pub job_id: String,
    pub permit: QueuePermit,
}
