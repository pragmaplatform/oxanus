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

pub(crate) struct ExecutionOutcome<ET> {
    pub(crate) result: Result<(), ExecutionError<ET>>,
    pub(crate) duration_ms: u64,
}

pub async fn run<DT, ET>(
    config: Arc<Config<DT, ET>>,
    worker: BoxedProcessable<ET>,
    envelope: &mut JobEnvelope,
) -> Result<ExecutionOutcome<ET>, OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    config.storage.internal.set_started_at(envelope).await?;
    let policy = execution_policy(&worker, 0, envelope);

    tracing::info!(
        job_id = envelope.id,
        queue = envelope.queue,
        worker = envelope.job.name,
        latency_ms = envelope.meta.latency_millis(),
        "Job started"
    );
    let start = std::time::Instant::now();
    let job_contexts = vec![job_context(&config.storage, envelope)];

    let result = run_process(worker, job_contexts, envelope).await;

    let duration = start.elapsed();
    let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    let is_err = !matches!(result, ExecutionResult::NotPanic(Ok(_)));
    tracing::info!(
        job_id = envelope.id,
        queue = envelope.queue,
        job = envelope.job.name,
        success = !is_err,
        duration = duration_ms,
        retries = envelope.meta.retries,
        "Job finished"
    );

    let result = finish_job_result(config.as_ref(), result, envelope, &policy).await;
    Ok(ExecutionOutcome {
        result,
        duration_ms,
    })
}

pub async fn run_batch<DT, ET>(
    config: Arc<Config<DT, ET>>,
    worker: BoxedProcessable<ET>,
    envelopes: &mut [JobEnvelope],
) -> Result<ExecutionOutcome<ET>, OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    if envelopes.is_empty() {
        return Ok(ExecutionOutcome {
            result: Ok(()),
            duration_ms: 0,
        });
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
        .map(|(index, envelope)| execution_policy(&worker, index, envelope))
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
    let job_contexts = job_contexts(&config.storage, envelopes);

    let result = run_process(worker, job_contexts, first_envelope).await;

    let duration = start.elapsed();
    let duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    let is_err = !matches!(result, ExecutionResult::NotPanic(Ok(_)));
    tracing::info!(
        batch_size = envelopes.len(),
        queue = queue,
        worker = worker_name,
        success = !is_err,
        duration = duration_ms,
        "Job batch finished"
    );

    let result = finish_batch_result(config.as_ref(), result, envelopes, &policies).await;
    Ok(ExecutionOutcome {
        result,
        duration_ms,
    })
}

struct JobExecutionPolicy {
    max_retries: u32,
    retry_delay: u64,
}

fn execution_policy<ET>(
    worker: &BoxedProcessable<ET>,
    index: usize,
    envelope: &JobEnvelope,
) -> JobExecutionPolicy
where
    ET: std::error::Error + Send + Sync + 'static,
{
    JobExecutionPolicy {
        max_retries: worker.max_retries(index),
        retry_delay: worker.retry_delay(index, envelope.meta.retries),
    }
}

async fn run_process<ET>(
    worker: BoxedProcessable<ET>,
    job_contexts: Vec<JobContext>,
    envelope: &JobEnvelope,
) -> ExecutionResult<ET>
where
    ET: std::error::Error + Send + Sync + 'static,
{
    match AssertUnwindSafe(process(worker, job_contexts, envelope))
        .catch_unwind()
        .await
    {
        Ok(result) => ExecutionResult::NotPanic(result),
        Err(panic) => ExecutionResult::Panic(panic_message(panic)),
    }
}

fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic occurred".to_string()
    }
}

async fn finish_job_result<DT, ET>(
    config: &Config<DT, ET>,
    result: ExecutionResult<ET>,
    envelope: &JobEnvelope,
    policy: &JobExecutionPolicy,
) -> Result<(), ExecutionError<ET>>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    match result {
        ExecutionResult::NotPanic(Ok(())) => {
            if let Err(e) = config.storage.internal.finish_with_success(envelope).await {
                tracing::error!("Failed to finish job: {}", e);
            }
            Ok(())
        }
        ExecutionResult::NotPanic(Err(e)) => {
            let retry_delay = retry_delay(config, &e, envelope, policy);

            #[cfg(feature = "sentry")]
            sentry_core::capture_error(&e);

            tracing::error!(
                job_id = envelope.id,
                queue = envelope.queue,
                worker = envelope.job.name,
                "Job failed"
            );

            handle_err(
                config,
                &e.to_string(),
                envelope,
                retry_delay,
                policy.max_retries,
            )
            .await;

            Err(ExecutionError::NotPanic(e))
        }
        ExecutionResult::Panic(panic_msg) => {
            #[cfg(feature = "sentry")]
            sentry_core::capture_message(&panic_msg, sentry_core::Level::Error);

            handle_err(
                config,
                &panic_msg,
                envelope,
                policy.retry_delay,
                policy.max_retries,
            )
            .await;

            Err(ExecutionError::Panic())
        }
    }
}

async fn finish_batch_result<DT, ET>(
    config: &Config<DT, ET>,
    result: ExecutionResult<ET>,
    envelopes: &[JobEnvelope],
    policies: &[JobExecutionPolicy],
) -> Result<(), ExecutionError<ET>>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    match result {
        ExecutionResult::NotPanic(Ok(())) => {
            if let Err(e) = config
                .storage
                .internal
                .finish_with_success_batch(envelopes)
                .await
            {
                tracing::error!("Failed to finish job batch: {}", e);
            }
            Ok(())
        }
        ExecutionResult::NotPanic(Err(e)) => {
            #[cfg(feature = "sentry")]
            sentry_core::capture_error(&e);

            if let Some(envelope) = envelopes.first() {
                tracing::error!(
                    batch_size = envelopes.len(),
                    queue = envelope.queue,
                    worker = envelope.job.name,
                    "Job batch failed"
                );
            }

            let err_msg = e.to_string();
            for (envelope, policy) in envelopes.iter().zip(policies.iter()) {
                handle_err(
                    config,
                    &err_msg,
                    envelope,
                    retry_delay(config, &e, envelope, policy),
                    policy.max_retries,
                )
                .await;
            }

            Err(ExecutionError::NotPanic(e))
        }
        ExecutionResult::Panic(panic_msg) => {
            #[cfg(feature = "sentry")]
            sentry_core::capture_message(&panic_msg, sentry_core::Level::Error);

            for (envelope, policy) in envelopes.iter().zip(policies.iter()) {
                handle_err(
                    config,
                    &panic_msg,
                    envelope,
                    policy.retry_delay,
                    policy.max_retries,
                )
                .await;
            }

            Err(ExecutionError::Panic())
        }
    }
}

fn retry_delay<DT, ET>(
    config: &Config<DT, ET>,
    error: &ET,
    envelope: &JobEnvelope,
    policy: &JobExecutionPolicy,
) -> u64
where
    ET: std::error::Error + Send + Sync + 'static,
{
    config
        .retry_delay_override
        .as_ref()
        .and_then(|f| f(error, envelope.meta.retries, policy.retry_delay))
        .unwrap_or(policy.retry_delay)
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

fn job_contexts(storage: &crate::Storage, envelopes: &[JobEnvelope]) -> Vec<JobContext> {
    envelopes
        .iter()
        .map(|envelope| job_context(storage, envelope))
        .collect()
}

async fn handle_err<DT, ET>(
    config: &Config<DT, ET>,
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
