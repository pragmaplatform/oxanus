use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use crate::context::JobState;
use crate::job_envelope::JobEnvelope;
use crate::worker::BoxedProcessable;
use crate::{Config, JobContext, OxanusError};

#[derive(Debug)]
enum ExecutionResult<ET> {
    NotPanic(Result<(), ET>),
    Panic(String),
}

pub(crate) enum ExecutionError<ET> {
    NotPanic(ET),
    Panic(),
}

pub async fn run<DT, ET>(
    config: Arc<Config<DT, ET>>,
    worker: BoxedProcessable<ET>,
    envelope: &mut JobEnvelope,
) -> Result<Result<(), ExecutionError<ET>>, OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    config.storage.internal.set_started_at(envelope).await?;
    let max_retries = worker.max_retries(0);
    let default_delay = worker.retry_delay(0, envelope.meta.retries);

    tracing::info!(
        job_id = envelope.id,
        queue = envelope.queue,
        worker = envelope.job.name,
        latency_ms = envelope.meta.latency_millis(),
        "Job started"
    );
    let start = std::time::Instant::now();
    let job_ctx = job_context(&config.storage, envelope);

    let result = match AssertUnwindSafe(process(worker, vec![job_ctx], envelope))
        .catch_unwind()
        .await
    {
        Ok(result) => ExecutionResult::NotPanic(result),
        Err(panic) => {
            let panic_msg = if let Some(s) = panic.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic occurred".to_string()
            };
            ExecutionResult::Panic(panic_msg)
        }
    };

    let duration = start.elapsed();
    let is_err = !matches!(result, ExecutionResult::NotPanic(Ok(_)));
    tracing::info!(
        job_id = envelope.id,
        queue = envelope.queue,
        job = envelope.job.name,
        success = !is_err,
        duration = duration.as_millis(),
        retries = envelope.meta.retries,
        "Job finished"
    );

    match result {
        ExecutionResult::NotPanic(result) => {
            match &result {
                Ok(()) => {
                    if let Err(e) = config.storage.internal.finish_with_success(envelope).await {
                        tracing::error!("Failed to finish job: {}", e);
                    }
                }
                Err(e) => {
                    let retry_delay = config
                        .retry_delay_override
                        .as_ref()
                        .and_then(|f| f(e, envelope.meta.retries, default_delay))
                        .unwrap_or(default_delay);

                    #[cfg(feature = "sentry")]
                    sentry_core::capture_error(e);

                    tracing::error!(
                        job_id = envelope.id,
                        queue = envelope.queue,
                        worker = envelope.job.name,
                        "Job failed"
                    );

                    handle_err(config, &e.to_string(), envelope, retry_delay, max_retries).await;
                }
            }

            Ok(result.map_err(ExecutionError::NotPanic))
        }
        ExecutionResult::Panic(panic_msg) => {
            #[cfg(feature = "sentry")]
            sentry_core::capture_message(&panic_msg, sentry_core::Level::Error);

            handle_err(config, &panic_msg, envelope, default_delay, max_retries).await;

            Ok(Err(ExecutionError::Panic()))
        }
    }
}

pub async fn run_batch<DT, ET>(
    config: Arc<Config<DT, ET>>,
    worker: BoxedProcessable<ET>,
    envelopes: &mut [JobEnvelope],
) -> Result<Result<(), ExecutionError<ET>>, OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    if envelopes.is_empty() {
        return Ok(Ok(()));
    }

    if worker.len() != envelopes.len() {
        return Err(OxanusError::GenericError(format!(
            "Batch worker has {} jobs but received {} envelopes",
            worker.len(),
            envelopes.len()
        )));
    }

    let policies: Vec<JobExecutionPolicy> = envelopes
        .iter()
        .enumerate()
        .map(|(index, envelope)| JobExecutionPolicy {
            max_retries: worker.max_retries(index),
            retry_delay: worker.retry_delay(index, envelope.meta.retries),
        })
        .collect();

    config
        .storage
        .internal
        .set_started_at_batch(envelopes)
        .await?;

    let first_envelope = envelopes
        .first()
        .expect("envelopes is not empty because it was checked above");
    let queue = first_envelope.queue.clone();
    let worker_name = first_envelope.job.name.clone();

    tracing::info!(
        batch_size = envelopes.len(),
        queue = queue,
        worker = worker_name,
        "Job batch started"
    );
    let start = std::time::Instant::now();
    let job_contexts = envelopes
        .iter()
        .map(|envelope| job_context(&config.storage, envelope))
        .collect();

    let result = match AssertUnwindSafe(process(worker, job_contexts, first_envelope))
        .catch_unwind()
        .await
    {
        Ok(result) => ExecutionResult::NotPanic(result),
        Err(panic) => {
            let panic_msg = if let Some(s) = panic.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic occurred".to_string()
            };
            ExecutionResult::Panic(panic_msg)
        }
    };

    let duration = start.elapsed();
    let is_err = !matches!(result, ExecutionResult::NotPanic(Ok(_)));
    tracing::info!(
        batch_size = envelopes.len(),
        queue = queue,
        worker = worker_name,
        success = !is_err,
        duration = duration.as_millis(),
        "Job batch finished"
    );

    match result {
        ExecutionResult::NotPanic(result) => {
            match &result {
                Ok(()) => {
                    if let Err(e) = config
                        .storage
                        .internal
                        .finish_with_success_batch(envelopes)
                        .await
                    {
                        tracing::error!("Failed to finish job batch: {}", e);
                    }
                }
                Err(e) => {
                    #[cfg(feature = "sentry")]
                    sentry_core::capture_error(e);

                    tracing::error!(
                        batch_size = envelopes.len(),
                        queue = queue,
                        worker = worker_name,
                        "Job batch failed"
                    );

                    for (envelope, policy) in envelopes.iter().zip(policies.iter()) {
                        let retry_delay = config
                            .retry_delay_override
                            .as_ref()
                            .and_then(|f| f(e, envelope.meta.retries, policy.retry_delay))
                            .unwrap_or(policy.retry_delay);

                        handle_err(
                            Arc::clone(&config),
                            &e.to_string(),
                            envelope,
                            retry_delay,
                            policy.max_retries,
                        )
                        .await;
                    }
                }
            }

            Ok(result.map_err(ExecutionError::NotPanic))
        }
        ExecutionResult::Panic(panic_msg) => {
            #[cfg(feature = "sentry")]
            sentry_core::capture_message(&panic_msg, sentry_core::Level::Error);

            for (envelope, policy) in envelopes.iter().zip(policies.iter()) {
                handle_err(
                    Arc::clone(&config),
                    &panic_msg,
                    envelope,
                    policy.retry_delay,
                    policy.max_retries,
                )
                .await;
            }

            Ok(Err(ExecutionError::Panic()))
        }
    }
}

struct JobExecutionPolicy {
    max_retries: u32,
    retry_delay: u64,
}

#[cfg_attr(feature = "tracing-instrument", tracing::instrument(skip_all, name = "job", fields(
    job_id = envelope.id,
    queue = envelope.queue,
    worker = envelope.job.name,
    args = %envelope.job.args,
    retries = envelope.meta.retries,
    latency_ms = envelope.meta.latency_millis(),
    success = false,
)))]
async fn process<ET>(
    worker: BoxedProcessable<ET>,
    job_contexts: Vec<JobContext>,
    #[cfg_attr(not(feature = "tracing-instrument"), allow(unused_variables))]
    envelope: &JobEnvelope,
) -> Result<(), ET>
where
    ET: std::error::Error + Send + Sync + 'static,
{
    #[cfg(feature = "tracing-instrument")]
    let span = tracing::Span::current();

    let result = worker.process(job_contexts).await;

    #[cfg(feature = "tracing-instrument")]
    span.record("success", result.is_ok());

    result
}

fn job_context(storage: &crate::Storage, envelope: &JobEnvelope) -> JobContext {
    JobContext {
        meta: envelope.meta.clone(),
        state: JobState::new(
            storage.clone(),
            envelope.id.clone(),
            envelope.meta.state.clone(),
        ),
    }
}

async fn handle_err<DT, ET>(
    config: Arc<Config<DT, ET>>,
    err_msg: &str,
    envelope: &JobEnvelope,
    retry_delay: u64,
    max_retries: u32,
) where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    if envelope.meta.retries < max_retries {
        if let Err(e) = config.storage.internal.finish_with_failure(envelope).await {
            tracing::error!("Failed to finish job: {}", e);
        }
        if let Err(e) = config
            .storage
            .internal
            .retry_in(envelope.id.clone(), retry_delay, err_msg.to_string())
            .await
        {
            tracing::error!("Failed to retry job: {}", e);
        }
    } else {
        tracing::error!(
            "Job {} failed after {} retries: {}",
            envelope.id,
            max_retries,
            err_msg
        );
        if let Err(e) = config
            .storage
            .internal
            .kill(envelope, err_msg.to_string())
            .await
        {
            tracing::error!("Failed to kill job: {}", e);
        }
    }
}
