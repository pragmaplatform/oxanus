use serde::{Deserialize, Serialize};
use std::{
    hash::{Hash, Hasher},
    time::Duration,
};

pub trait Queue: Send + Sync + Serialize {
    fn key(&self) -> String {
        match Self::to_config().kind {
            QueueKind::Static { key } => key,
            QueueKind::Dynamic { prefix, .. } => {
                let value = serde_json::to_value(self).unwrap_or_default();
                format!("{}#{}", prefix, value_to_queue_key(value))
            }
        }
    }
    fn to_config() -> QueueConfig;
    fn config(&self) -> QueueConfig {
        Self::to_config()
    }
}

#[derive(Debug, Clone)]
pub struct QueueConfig {
    pub kind: QueueKind,
    pub concurrency: QueueConcurrency,
    pub throttle: Option<QueueThrottle>,
}

impl PartialEq for QueueConfig {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Eq for QueueConfig {}

impl Hash for QueueConfig {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.kind.hash(state);
    }
}

impl QueueConfig {
    pub fn as_dynamic(prefix: impl Into<String>) -> Self {
        Self {
            kind: QueueKind::Dynamic {
                prefix: prefix.into(),
                discovery_interval: Duration::from_millis(500),
            },
            concurrency: QueueConcurrency::Fixed(1),
            throttle: None,
        }
    }

    pub fn as_static(key: impl Into<String>) -> Self {
        Self {
            kind: QueueKind::Static { key: key.into() },
            concurrency: QueueConcurrency::Fixed(1),
            throttle: None,
        }
    }

    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = QueueConcurrency::Fixed(concurrency);
        self
    }

    /// Sets the default concurrency used by the runtime queue configuration.
    ///
    /// Runtime queue config is persisted in Redis, so this value is used when a
    /// queue does not have an existing runtime override yet.
    pub fn dynamic_concurrency(mut self, default_concurrency: usize) -> Self {
        self.concurrency = QueueConcurrency::Dynamic {
            default: default_concurrency,
        };
        self
    }

    pub fn throttle(mut self, throttle: QueueThrottle) -> Self {
        self.throttle = Some(throttle);
        self
    }

    pub fn discovery_interval(mut self, interval: Duration) -> Self {
        if let QueueKind::Dynamic {
            discovery_interval, ..
        } = &mut self.kind
        {
            *discovery_interval = interval;
        }
        self
    }

    pub fn static_key(&self) -> Option<String> {
        match &self.kind {
            QueueKind::Static { key } => Some(key.clone()),
            QueueKind::Dynamic { .. } => None,
        }
    }

    pub fn key_or_prefix(&self) -> String {
        match &self.kind {
            QueueKind::Static { key } => key.clone(),
            QueueKind::Dynamic { prefix, .. } => prefix.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum QueueConcurrency {
    Fixed(usize),
    Dynamic { default: usize },
}

impl QueueConcurrency {
    pub fn default_concurrency(self) -> usize {
        match self {
            Self::Fixed(concurrency) => concurrency,
            Self::Dynamic { default } => default,
        }
    }

    pub fn is_dynamic(self) -> bool {
        matches!(self, Self::Dynamic { .. })
    }

    pub(crate) fn stored_runtime_default(self) -> QueueRuntimeConfig {
        QueueRuntimeConfig {
            concurrency: self.is_dynamic().then_some(self.default_concurrency()),
            state: QueueState::Active,
        }
    }

    pub(crate) fn effective_runtime_config(
        self,
        runtime_config: QueueRuntimeConfig,
    ) -> QueueRuntimeConfig {
        let concurrency = match self {
            Self::Fixed(concurrency) => concurrency,
            Self::Dynamic { default } => runtime_config.concurrency.unwrap_or(default),
        };

        QueueRuntimeConfig {
            concurrency: Some(concurrency),
            state: runtime_config.state,
        }
    }
}

#[derive(Debug, Clone)]
pub enum QueueKind {
    Static {
        key: String,
    },
    Dynamic {
        prefix: String,
        discovery_interval: Duration,
    },
}

impl PartialEq for QueueKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (QueueKind::Static { key: k1 }, QueueKind::Static { key: k2 }) => k1 == k2,
            (QueueKind::Dynamic { prefix: p1, .. }, QueueKind::Dynamic { prefix: p2, .. }) => {
                p1 == p2
            }
            _ => false,
        }
    }
}

impl Eq for QueueKind {}

impl Hash for QueueKind {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            QueueKind::Static { key } => key.hash(state),
            QueueKind::Dynamic { prefix, .. } => prefix.hash(state),
        }
    }
}

impl QueueKind {
    pub fn is_dynamic(&self) -> bool {
        matches!(self, QueueKind::Dynamic { .. })
    }

    pub fn is_static(&self) -> bool {
        matches!(self, QueueKind::Static { .. })
    }
}

#[derive(Debug, Clone)]
pub struct QueueThrottle {
    pub window_ms: i64,
    pub limit: u64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueState {
    #[default]
    Active,
    Paused,
}

impl QueueState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Paused => "Paused",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct QueueRuntimeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<usize>,
    #[serde(default)]
    pub state: QueueState,
}

impl QueueRuntimeConfig {
    pub fn new(concurrency: usize) -> Self {
        Self {
            concurrency: Some(concurrency),
            state: QueueState::Active,
        }
    }

    pub fn with_defaults(mut self, defaults: Self) -> Self {
        self.concurrency = self.concurrency.or(defaults.concurrency);
        self
    }
}

fn value_to_queue_key(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "".to_string(),
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Array(a) => a
            .into_iter()
            .map(value_to_queue_key)
            .collect::<Vec<String>>()
            .join(":"),
        serde_json::Value::Object(object) => object
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, value_to_queue_key(v)))
            .collect::<Vec<String>>()
            .join(":"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct TestStaticQueue;

    impl Queue for TestStaticQueue {
        fn to_config() -> QueueConfig {
            QueueConfig::as_static("test_static_queue")
        }
    }

    #[derive(Serialize)]
    struct TestDynamicQueue {
        name: String,
        age: u32,
        is_student: bool,
    }

    impl Queue for TestDynamicQueue {
        fn to_config() -> QueueConfig {
            QueueConfig::as_dynamic("test_dynamic_queue")
        }
    }

    #[test]
    fn test_queue_key() {
        let static_queue = TestStaticQueue;
        let dynamic_queue = TestDynamicQueue {
            name: "John".to_string(),
            age: 30,
            is_student: true,
        };

        assert_eq!(static_queue.key(), "test_static_queue");
        assert_eq!(
            dynamic_queue.key(),
            "test_dynamic_queue#name=John:age=30:is_student=true"
        );
    }

    #[cfg(feature = "macros")]
    #[test]
    fn test_define_queue_with_macro() {
        use crate as oxana; // needed for unit test

        #[derive(oxana::Registry)]
        #[allow(dead_code)]
        struct ComponentRegistry(oxana::ComponentRegistry<()>);

        #[derive(Serialize, oxana::Queue)]
        struct DefaultQueue;

        assert_eq!(DefaultQueue.key(), "default_queue");
        assert_eq!(
            DefaultQueue.config().concurrency,
            QueueConcurrency::Fixed(1)
        );

        #[derive(Serialize, oxana::Queue)]
        #[oxana(key = "static_queue")]
        struct QueueWithKey;

        assert_eq!(QueueWithKey.key(), "static_queue");
        assert_eq!(
            QueueWithKey.config().concurrency,
            QueueConcurrency::Fixed(1)
        );

        #[derive(Serialize, oxana::Queue)]
        #[oxana(concurrency = 2)]
        struct QueueWithConcurrency;

        assert_eq!(QueueWithConcurrency.key(), "queue_with_concurrency");
        assert_eq!(
            QueueWithConcurrency.config().concurrency,
            QueueConcurrency::Fixed(2)
        );

        #[derive(Serialize, oxana::Queue)]
        #[oxana(concurrency = Fixed(2))]
        struct QueueWithFixedConcurrency;

        assert_eq!(
            QueueWithFixedConcurrency.config().concurrency,
            QueueConcurrency::Fixed(2)
        );

        #[derive(Serialize, oxana::Queue)]
        #[oxana(concurrency = Dynamic(2))]
        struct QueueWithDynamicConcurrency;

        assert_eq!(
            QueueWithDynamicConcurrency.config().concurrency,
            QueueConcurrency::Dynamic { default: 2 }
        );

        #[derive(Serialize, oxana::Queue)]
        #[oxana(concurrency = 2)]
        #[oxana(throttle(window_ms = 3, limit = 4))]
        struct QueueWithThrottle;

        assert_eq!(QueueWithThrottle.key(), "queue_with_throttle");
        assert_eq!(
            QueueWithThrottle.config().concurrency,
            QueueConcurrency::Fixed(2)
        );
        assert_eq!(QueueWithThrottle.config().throttle.unwrap().window_ms, 3);
        assert_eq!(QueueWithThrottle.config().throttle.unwrap().limit, 4);

        #[derive(Serialize, oxana::Queue)]
        #[oxana(key = "static_queue_key")]
        #[oxana(concurrency = 2)]
        struct QueueWithKeyAndConcurrency;

        assert_eq!(QueueWithKeyAndConcurrency.key(), "static_queue_key");
        assert_eq!(
            QueueWithKeyAndConcurrency.config().concurrency,
            QueueConcurrency::Fixed(2)
        );

        #[derive(Serialize, oxana::Queue)]
        #[oxana(key = "static_queue_key", concurrency = 3)]
        struct QueueWithKeyAndConcurrency1 {}

        assert_eq!(QueueWithKeyAndConcurrency1 {}.key(), "static_queue_key");
        assert_eq!(
            QueueWithKeyAndConcurrency1 {}.config().concurrency,
            QueueConcurrency::Fixed(3)
        );

        #[derive(Serialize, oxana::Queue)]
        #[oxana(prefix = "dyn_queue", concurrency = 2, discovery_interval_ms = 250)]
        struct DynQueue {
            i: i32,
        }

        assert_eq!(DynQueue { i: 2 }.key(), "dyn_queue#i=2");
        assert_eq!(
            DynQueue::to_config().concurrency,
            QueueConcurrency::Fixed(2)
        );
        let QueueKind::Dynamic {
            discovery_interval, ..
        } = DynQueue::to_config().kind
        else {
            panic!("expected dynamic queue");
        };
        assert_eq!(discovery_interval, Duration::from_millis(250));

        #[derive(Serialize, oxana::Queue)]
        #[oxana(key = "runtime_queue", concurrency = Dynamic(4))]
        struct RuntimeQueue;

        assert_eq!(RuntimeQueue.key(), "runtime_queue");
        assert_eq!(
            RuntimeQueue.config().concurrency,
            QueueConcurrency::Dynamic { default: 4 }
        );
    }

    #[test]
    fn runtime_config_can_store_state_without_concurrency() {
        let config = QueueRuntimeConfig {
            state: QueueState::Paused,
            ..QueueRuntimeConfig::default()
        };

        assert_eq!(
            serde_json::to_string(&config).unwrap(),
            r#"{"state":"paused"}"#
        );
        assert_eq!(config.concurrency.unwrap_or(4), 4);
        assert_eq!(
            config.with_defaults(QueueRuntimeConfig::new(4)),
            QueueRuntimeConfig {
                concurrency: Some(4),
                state: QueueState::Paused,
            }
        );
    }
}
