use crate::context::JobContext;
use crate::error::OxanusError;
use crate::worker::{BoxedProcessable, FromContext, Processable};

pub type BatchFactory<DT, ET> =
    fn(Vec<serde_json::Value>, &DT) -> Result<BoxedProcessable<ET>, OxanusError>;

pub struct BatchProcessorConfig<DT, ET> {
    pub worker_name: String,
    pub batch_size: usize,
    pub batch_linger_ms: u64,
    pub factory: BatchFactory<DT, ET>,
}

#[async_trait::async_trait]
pub trait BatchProcessor: Send + Sync + Sized {
    type Item: serde::de::DeserializeOwned + Send + Sync;
    type Error: std::error::Error + Send + Sync;

    async fn process_batch(&self, items: &[Self::Item], ctx: &JobContext)
        -> Result<(), Self::Error>;

    fn max_retries(&self) -> u32 {
        2
    }

    fn retry_delay(&self, retries: u32) -> u64 {
        u64::pow(5, retries + 2)
    }

    fn batch_size() -> usize;
    fn batch_linger_ms() -> u64;
}

pub(crate) struct BoundBatch<B: BatchProcessor> {
    pub processor: B,
    pub items: Vec<B::Item>,
}

#[async_trait::async_trait]
impl<B> Processable for BoundBatch<B>
where
    B: BatchProcessor + Send + Sync + 'static,
    B::Item: Send + Sync + 'static,
{
    type Error = B::Error;

    async fn process(&self, ctx: &JobContext) -> Result<(), Self::Error> {
        self.processor.process_batch(&self.items, ctx).await
    }

    fn max_retries(&self) -> u32 {
        self.processor.max_retries()
    }

    fn retry_delay(&self, retries: u32) -> u64 {
        self.processor.retry_delay(retries)
    }
}

pub fn batch_factory<B, DT, ET>(
    args: Vec<serde_json::Value>,
    ctx: &DT,
) -> Result<BoxedProcessable<ET>, OxanusError>
where
    B: BatchProcessor<Error = ET> + FromContext<DT> + 'static,
    B::Item: 'static,
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let items: Vec<B::Item> = args
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()?;
    let processor = B::from_context(ctx);
    Ok(Box::new(BoundBatch { processor, items }))
}
