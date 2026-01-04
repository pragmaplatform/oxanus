use serde::Serialize;
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
    pub concurrency: usize,
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
                sleep_period: Duration::from_millis(500),
            },
            concurrency: 1,
            throttle: None,
        }
    }

    pub fn as_static(key: impl Into<String>) -> Self {
        Self {
            kind: QueueKind::Static { key: key.into() },
            concurrency: 1,
            throttle: None,
        }
    }

    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    pub fn throttle(mut self, throttle: QueueThrottle) -> Self {
        self.throttle = Some(throttle);
        self
    }
}

#[derive(Debug, Clone)]
pub enum QueueKind {
    Static {
        key: String,
    },
    Dynamic {
        prefix: String,
        sleep_period: Duration,
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
        use crate as oxanus; // needed for unit test

        #[derive(Serialize, oxanus::Queue)]
        struct DefaultQueue;

        assert_eq!(DefaultQueue.key(), "default_queue");
        assert_eq!(DefaultQueue.config().concurrency, 1);

        #[derive(Serialize, oxanus::Queue)]
        #[oxanus(key = "static_queue")]
        struct QueueWithKey;

        assert_eq!(QueueWithKey.key(), "static_queue");
        assert_eq!(QueueWithKey.config().concurrency, 1);

        #[derive(Serialize, oxanus::Queue)]
        #[oxanus(concurrency = 2)]
        struct QueueWithConcurrency;

        assert_eq!(QueueWithConcurrency.key(), "queue_with_concurrency");
        assert_eq!(QueueWithConcurrency.config().concurrency, 2);

        #[derive(Serialize, oxanus::Queue)]
        #[oxanus(concurrency = 2)]
        #[oxanus(throttle(window_ms = 3, limit = 4))]
        struct QueueWithThrottle;

        assert_eq!(QueueWithThrottle.key(), "queue_with_throttle");
        assert_eq!(QueueWithThrottle.config().concurrency, 2);
        assert_eq!(QueueWithThrottle.config().throttle.unwrap().window_ms, 3);
        assert_eq!(QueueWithThrottle.config().throttle.unwrap().limit, 4);

        #[derive(Serialize, oxanus::Queue)]
        #[oxanus(key = "static_queue_key")]
        #[oxanus(concurrency = 2)]
        struct QueueWithKeyAndConcurrency;

        assert_eq!(QueueWithKeyAndConcurrency.key(), "static_queue_key");
        assert_eq!(QueueWithKeyAndConcurrency.config().concurrency, 2);

        #[derive(Serialize, oxanus::Queue)]
        #[oxanus(key = "static_queue_key", concurrency = 3)]
        struct QueueWithKeyAndConcurrency1 {}

        assert_eq!(QueueWithKeyAndConcurrency1 {}.key(), "static_queue_key");
        assert_eq!(QueueWithKeyAndConcurrency1 {}.config().concurrency, 3);

        #[derive(Serialize, oxanus::Queue)]
        #[oxanus(prefix = "dyn_queue", concurrency = 2)]
        struct DynQueue {
            i: i32,
        }

        assert_eq!(DynQueue { i: 2 }.key(), "dyn_queue#i=2");
        assert_eq!(DynQueue::to_config().concurrency, 2);
    }
}
