use deadpool_redis::Hook;
use std::time::Duration;

use crate::{OxanaError, Storage, storage_internal::StorageInternal};

#[must_use]
pub struct StorageBuilder {
    namespace: Option<String>,
    max_pool_size: Option<usize>,
    timeouts: Option<StorageBuilderTimeouts>,
}

#[derive(Clone, Copy)]
pub struct StorageBuilderTimeouts {
    pub wait: Option<std::time::Duration>,
    pub create: Option<std::time::Duration>,
    pub recycle: Option<std::time::Duration>,
}

impl StorageBuilderTimeouts {
    pub fn new(timeout: std::time::Duration) -> Self {
        Self {
            wait: Some(timeout),
            create: Some(timeout),
            recycle: Some(timeout),
        }
    }
}

impl Default for StorageBuilderTimeouts {
    fn default() -> Self {
        Self {
            wait: Some(std::time::Duration::from_millis(300)),
            create: Some(std::time::Duration::from_millis(300)),
            recycle: Some(std::time::Duration::from_millis(300)),
        }
    }
}

impl From<StorageBuilderTimeouts> for deadpool_redis::Timeouts {
    fn from(value: StorageBuilderTimeouts) -> Self {
        deadpool_redis::Timeouts {
            wait: value.wait,
            create: value.create,
            recycle: value.recycle,
        }
    }
}

impl Default for StorageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBuilder {
    pub fn new() -> Self {
        Self {
            namespace: None,
            max_pool_size: None,
            timeouts: None,
        }
    }

    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    pub fn max_pool_size(mut self, max_pool_size: usize) -> Self {
        self.max_pool_size = Some(max_pool_size);
        self
    }

    pub fn timeouts(mut self, timeouts: StorageBuilderTimeouts) -> Self {
        self.timeouts = Some(timeouts);
        self
    }

    pub fn build_from_redis_url(self, url: impl Into<String>) -> Result<Storage, OxanaError> {
        let pool = Self::build_redis_pool(
            url,
            self.max_pool_size.unwrap_or(50),
            self.timeouts.unwrap_or_default(),
        )?;

        Ok(Storage {
            internal: StorageInternal::new(pool, self.namespace),
        })
    }

    pub fn build_from_redis_urls(
        self,
        url: impl Into<String>,
        stats_url: impl Into<String>,
    ) -> Result<Storage, OxanaError> {
        let max_pool_size = self.max_pool_size.unwrap_or(50);
        let timeouts = self.timeouts.unwrap_or_default();
        let pool = Self::build_redis_pool(url, max_pool_size, timeouts)?;
        let stats_pool = Self::build_redis_pool(stats_url, max_pool_size, timeouts)?;

        Ok(Storage {
            internal: StorageInternal::with_stats_pool(pool, stats_pool, self.namespace),
        })
    }

    fn build_redis_pool(
        url: impl Into<String>,
        max_pool_size: usize,
        timeouts: StorageBuilderTimeouts,
    ) -> Result<deadpool_redis::Pool, OxanaError> {
        let mut cfg = deadpool_redis::Config::from_url(url);
        cfg.pool = Some(deadpool_redis::PoolConfig {
            max_size: max_pool_size,
            timeouts: timeouts.into(),
            queue_mode: Default::default(),
        });

        Ok(cfg
            .builder()?
            .post_create(Hook::sync_fn(|conn, _| {
                // redis 1.0 introduced a default 500ms response_timeout on
                // MultiplexedConnection. This causes blocking Redis commands
                // (BLMOVE, BLPOP, etc.) to time out prematurely. Disable it
                // so that command-level timeouts govern blocking behavior.
                conn.set_response_timeout(Duration::from_secs(60));
                Ok(())
            }))
            .runtime(deadpool_redis::Runtime::Tokio1)
            .build()?)
    }

    pub fn build_from_env(self) -> Result<Storage, OxanaError> {
        self.build_from_env_var("REDIS_URL")
    }

    pub fn build_from_env_var(self, var_name: &str) -> Result<Storage, OxanaError> {
        let url = std::env::var(var_name).unwrap_or_else(|_| panic!("{var_name} is not set"));
        match std::env::var("REDIS_STATS_URL") {
            Ok(stats_url) => self.build_from_redis_urls(url, stats_url),
            Err(_) => self.build_from_redis_url(url),
        }
    }

    pub fn build_from_pool(self, pool: deadpool_redis::Pool) -> Result<Storage, OxanaError> {
        let internal = StorageInternal::new(pool, self.namespace);
        Ok(Storage { internal })
    }

    pub fn build_from_pools(
        self,
        pool: deadpool_redis::Pool,
        stats_pool: deadpool_redis::Pool,
    ) -> Result<Storage, OxanaError> {
        let internal = StorageInternal::with_stats_pool(pool, stats_pool, self.namespace);
        Ok(Storage { internal })
    }
}

#[cfg(test)]
mod tests {
    use super::StorageBuilder;

    #[test]
    fn build_from_redis_url_uses_primary_pool_for_stats() {
        let storage = StorageBuilder::new()
            .build_from_redis_url("redis://127.0.0.1/0")
            .expect("storage should build");

        assert!(!storage.internal.has_dedicated_stats_pool());
    }

    #[test]
    fn build_from_redis_urls_uses_dedicated_stats_pool() {
        let storage = StorageBuilder::new()
            .build_from_redis_urls("redis://127.0.0.1/0", "redis://127.0.0.1/1")
            .expect("storage should build");

        assert!(storage.internal.has_dedicated_stats_pool());
    }
}
