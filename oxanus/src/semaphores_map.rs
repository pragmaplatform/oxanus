use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

pub struct SemaphoresMap {
    default_permits: usize,
    inner: Mutex<HashMap<String, (Arc<Semaphore>, usize)>>,
}

impl SemaphoresMap {
    pub fn new(permits: usize) -> Self {
        Self {
            default_permits: permits,
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_or_create(&self, key: String) -> Arc<Semaphore> {
        let mut map = self.inner.lock().await;
        let (sem, _) = map.entry(key).or_insert_with(|| {
            (
                Arc::new(Semaphore::new(self.default_permits)),
                self.default_permits,
            )
        });
        Arc::clone(sem)
    }

    pub async fn add_permits(&self, key: &str, additional: usize) {
        let mut map = self.inner.lock().await;
        if let Some((sem, total)) = map.get_mut(key) {
            sem.add_permits(additional);
            *total += additional;
        }
    }

    pub async fn busy_count(&self) -> usize {
        let map = self.inner.lock().await;
        map.values()
            .map(|(sem, total)| total - sem.available_permits())
            .sum()
    }
}
