// cargo run --package oxanus-web --example web

use serde::Serialize;

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

mod demo {
    pub(crate) mod crm {
        pub(crate) mod customers {
            use crate::{ComponentRegistry, WorkerContext, WorkerError};
            use serde::{Deserialize, Serialize};

            #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
            #[oxanus(on_demand)]
            pub(crate) struct SyncCustomerProfileJob {
                pub(crate) duration_ms: u64,
            }

            #[derive(oxanus::Worker)]
            #[oxanus(max_retries = 0)]
            struct SyncCustomerProfileWorker;

            impl SyncCustomerProfileWorker {
                async fn process(
                    &self,
                    job: SyncCustomerProfileJob,
                    _ctx: &oxanus::JobContext,
                ) -> Result<(), WorkerError> {
                    tokio::time::sleep(std::time::Duration::from_millis(job.duration_ms)).await;
                    Ok(())
                }
            }
        }
    }

    pub(crate) mod billing {
        pub(crate) mod invoices {
            use crate::{ComponentRegistry, WorkerContext, WorkerError};
            use serde::{Deserialize, Serialize};

            #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
            #[oxanus(on_demand)]
            pub(crate) struct GenerateInvoiceJob {
                pub(crate) duration_ms: u64,
            }

            #[derive(oxanus::Worker)]
            #[oxanus(max_retries = 0)]
            struct GenerateInvoiceWorker;

            impl GenerateInvoiceWorker {
                async fn process(
                    &self,
                    job: GenerateInvoiceJob,
                    _ctx: &oxanus::JobContext,
                ) -> Result<(), WorkerError> {
                    tokio::time::sleep(std::time::Duration::from_millis(job.duration_ms)).await;
                    Ok(())
                }
            }
        }
    }

    pub(crate) mod messaging {
        pub(crate) mod receipts {
            use crate::{ComponentRegistry, WorkerContext, WorkerError};
            use serde::{Deserialize, Serialize};

            #[derive(Debug, Serialize, Deserialize, oxanus::Job)]
            pub(crate) struct SendReceiptEmailJob {
                pub(crate) duration_ms: u64,
            }

            #[derive(oxanus::Worker)]
            #[oxanus(max_retries = 0)]
            struct SendReceiptEmailWorker;

            impl SendReceiptEmailWorker {
                async fn process(
                    &self,
                    job: SendReceiptEmailJob,
                    _ctx: &oxanus::JobContext,
                ) -> Result<(), WorkerError> {
                    tokio::time::sleep(std::time::Duration::from_millis(job.duration_ms)).await;
                    Ok(())
                }
            }
        }
    }
}

use demo::billing::invoices::GenerateInvoiceJob;
use demo::crm::customers::SyncCustomerProfileJob;
use demo::messaging::receipts::SendReceiptEmailJob;

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "default", concurrency = 5)]
struct DefaultQueue;

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "priority", concurrency = 5)]
struct PriorityQueue;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config =
        ComponentRegistry::build_config(&storage).with_graceful_shutdown(tokio::signal::ctrl_c());
    let catalog = config.catalog();
    let worker_ctx = oxanus::ContextValue::new(WorkerContext {});

    seed_sample_jobs(&storage).await?;

    let mut worker = tokio::spawn(async move {
        if let Err(err) = oxanus::run(config, worker_ctx).await {
            eprintln!("Oxanus worker exited with error: {err}");
        }
    });

    let base_path = "/oxanus";
    let oxanus_state = oxanus_web::OxanusWebState::new(storage, catalog, base_path.to_string());

    let app = axum::Router::new().nest(base_path, oxanus_web::router(oxanus_state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Oxanus web UI available at http://localhost:3000{base_path}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    if !worker.is_finished() {
        match tokio::time::timeout(std::time::Duration::from_secs(2), &mut worker).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) if err.is_cancelled() => {}
            Ok(Err(err)) => return Err(Box::new(err) as Box<dyn std::error::Error>),
            Err(_) => {
                worker.abort();
                match worker.await {
                    Ok(()) => {}
                    Err(err) if err.is_cancelled() => {}
                    Err(err) => return Err(Box::new(err) as Box<dyn std::error::Error>),
                }
            }
        }
    }

    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        eprintln!("Failed to listen for Ctrl-C: {err}");
    }
    println!("Shutting down Oxanus web example");
}

async fn seed_sample_jobs(storage: &oxanus::Storage) -> Result<(), oxanus::OxanusError> {
    for _ in 0..3 {
        storage
            .enqueue(PriorityQueue, SyncCustomerProfileJob { duration_ms: 0 })
            .await?;
    }
    for _ in 0..2 {
        storage
            .enqueue(DefaultQueue, SyncCustomerProfileJob { duration_ms: 0 })
            .await?;
    }
    for _ in 0..2 {
        storage
            .enqueue(DefaultQueue, GenerateInvoiceJob { duration_ms: 0 })
            .await?;
    }
    for _ in 0..3 {
        storage
            .enqueue(DefaultQueue, SendReceiptEmailJob { duration_ms: 0 })
            .await?;
    }

    for duration_ms in [25, 50, 100, 250, 500, 1000, 2500, 1000] {
        storage
            .enqueue(PriorityQueue, SyncCustomerProfileJob { duration_ms })
            .await?;
    }
    for duration_ms in [25, 50, 100, 250, 500, 1000, 2500, 2000] {
        storage
            .enqueue(DefaultQueue, SyncCustomerProfileJob { duration_ms })
            .await?;
    }
    for duration_ms in [25, 50, 100, 250, 500, 1000, 2500, 2000] {
        storage
            .enqueue(DefaultQueue, GenerateInvoiceJob { duration_ms })
            .await?;
    }
    for duration_ms in [25, 50, 100, 250, 500, 1000, 2500, 2000] {
        storage
            .enqueue(DefaultQueue, SendReceiptEmailJob { duration_ms })
            .await?;
    }

    storage
        .enqueue(
            PriorityQueue,
            SyncCustomerProfileJob {
                duration_ms: seconds(15),
            },
        )
        .await?;
    storage
        .enqueue(
            DefaultQueue,
            SyncCustomerProfileJob {
                duration_ms: seconds(30),
            },
        )
        .await?;
    storage
        .enqueue(
            DefaultQueue,
            GenerateInvoiceJob {
                duration_ms: seconds(45),
            },
        )
        .await?;
    storage
        .enqueue(
            DefaultQueue,
            SendReceiptEmailJob {
                duration_ms: seconds(60),
            },
        )
        .await?;
    storage
        .enqueue(
            PriorityQueue,
            SyncCustomerProfileJob {
                duration_ms: seconds(75),
            },
        )
        .await?;
    storage
        .enqueue(
            DefaultQueue,
            SyncCustomerProfileJob {
                duration_ms: seconds(90),
            },
        )
        .await?;
    storage
        .enqueue(
            DefaultQueue,
            GenerateInvoiceJob {
                duration_ms: seconds(105),
            },
        )
        .await?;
    storage
        .enqueue(
            DefaultQueue,
            SendReceiptEmailJob {
                duration_ms: seconds(120),
            },
        )
        .await?;
    storage
        .enqueue(
            PriorityQueue,
            SyncCustomerProfileJob {
                duration_ms: seconds(135),
            },
        )
        .await?;
    storage
        .enqueue(
            DefaultQueue,
            SyncCustomerProfileJob {
                duration_ms: seconds(150),
            },
        )
        .await?;

    println!("Seeded sample jobs across default and priority queues");

    Ok(())
}

fn seconds(value: u64) -> u64 {
    value * 1000
}
