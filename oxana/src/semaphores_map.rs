use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{Mutex as TokioMutex, Notify};

use crate::queue::{QueueRuntimeConfig, QueueState};

pub struct QueueControlsMap {
    inner: TokioMutex<HashMap<String, Arc<QueueControl>>>,
}

impl QueueControlsMap {
    pub fn new() -> Self {
        Self {
            inner: TokioMutex::new(HashMap::new()),
        }
    }

    pub async fn get_or_create(
        &self,
        key: String,
        config: QueueRuntimeConfig,
    ) -> Arc<QueueControl> {
        let mut map = self.inner.lock().await;
        Arc::clone(
            map.entry(key)
                .or_insert_with(|| Arc::new(QueueControl::new(config))),
        )
    }

    pub async fn busy_count(&self) -> usize {
        let map = self.inner.lock().await;
        map.values().map(|control| control.active_count()).sum()
    }
}

pub struct QueueControl {
    inner: Mutex<QueueControlState>,
    notify: Notify,
}

struct QueueControlState {
    concurrency: usize,
    active: usize,
    queue_state: QueueState,
}

pub struct QueuePermit {
    control: Arc<QueueControl>,
}

impl std::fmt::Debug for QueuePermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueuePermit").finish_non_exhaustive()
    }
}

impl QueueControl {
    fn new(config: QueueRuntimeConfig) -> Self {
        Self {
            inner: Mutex::new(QueueControlState {
                concurrency: config.concurrency.unwrap_or(1),
                active: 0,
                queue_state: config.state,
            }),
            notify: Notify::new(),
        }
    }

    pub async fn acquire(self: &Arc<Self>) -> QueuePermit {
        loop {
            let notified = self.notify.notified();
            {
                let mut state = self.lock_state();
                if state.queue_state.is_active() && state.active < state.concurrency {
                    state.active += 1;
                    return QueuePermit {
                        control: Arc::clone(self),
                    };
                }
            }
            notified.await;
        }
    }

    pub fn apply_config(&self, config: QueueRuntimeConfig) {
        {
            let mut state = self.lock_state();
            if let Some(concurrency) = config.concurrency {
                state.concurrency = concurrency;
            }
            state.queue_state = config.state;
        }
        self.notify.notify_waiters();
    }

    fn active_count(&self) -> usize {
        self.lock_state().active
    }

    fn release(&self) {
        {
            let mut state = self.lock_state();
            state.active = state.active.saturating_sub(1);
        }
        self.notify.notify_waiters();
    }

    fn lock_state(&self) -> MutexGuard<'_, QueueControlState> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }
}

impl Drop for QueuePermit {
    fn drop(&mut self) {
        self.control.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, timeout};

    #[tokio::test]
    async fn paused_control_blocks_until_active() {
        let control = Arc::new(QueueControl::new(QueueRuntimeConfig {
            concurrency: Some(1),
            state: QueueState::Paused,
        }));

        assert!(
            timeout(Duration::from_millis(20), control.acquire())
                .await
                .is_err()
        );

        control.apply_config(QueueRuntimeConfig::new(1));

        assert!(
            timeout(Duration::from_millis(20), control.acquire())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn reducing_concurrency_waits_for_active_permits_to_finish() {
        let control = Arc::new(QueueControl::new(QueueRuntimeConfig::new(2)));

        let first = control.acquire().await;
        let second = control.acquire().await;

        control.apply_config(QueueRuntimeConfig::new(1));

        assert!(
            timeout(Duration::from_millis(20), control.acquire())
                .await
                .is_err()
        );

        drop(second);
        assert!(
            timeout(Duration::from_millis(20), control.acquire())
                .await
                .is_err()
        );

        drop(first);
        assert!(
            timeout(Duration::from_millis(20), control.acquire())
                .await
                .is_ok()
        );
    }
}
