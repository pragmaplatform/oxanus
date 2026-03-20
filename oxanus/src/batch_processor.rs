use crate::error::OxanusError;
use crate::worker::{BoxedWorker, Worker};

pub type BatchFactory<DT, ET> =
    fn(Vec<serde_json::Value>) -> Result<BoxedWorker<DT, ET>, OxanusError>;

pub struct BatchProcessorConfig<DT, ET> {
    pub worker_name: String,
    pub batch_size: usize,
    pub batch_linger_ms: u64,
    pub factory: BatchFactory<DT, ET>,
}

pub trait BatchProcessor: Worker + Sized {
    type Item: serde::de::DeserializeOwned + Send + Sync;

    fn from_args(args: Vec<serde_json::Value>) -> Result<Self, OxanusError>;
    fn batch_size() -> usize;
    fn batch_linger_ms() -> u64;
}

pub fn batch_factory<B, DT, ET>(
    args: Vec<serde_json::Value>,
) -> Result<BoxedWorker<DT, ET>, OxanusError>
where
    B: BatchProcessor<Context = DT, Error = ET> + 'static,
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let processor = B::from_args(args)?;
    Ok(Box::new(processor))
}
