use crate::{QueueConfig, Storage, worker_registry::WorkerConfig};

pub struct ComponentRegistry<DT> {
    /// `module_path!()`
    pub module_path: &'static str,
    /// `stringify!(MyStruct)`
    pub type_name: &'static str,
    pub definition: fn() -> ComponentDefinition<DT>,
}

pub enum ComponentDefinition<DT> {
    Queue(QueueConfig),
    Worker(WorkerConfig<DT>),
}

pub trait RegisterComponents {
    type Context: Clone + Send + Sync + 'static;

    fn register_components(storage: Storage) -> Storage;
}

/// Macro to create a component registry
pub use inventory::collect as create_component_registry;

/// Macro to register a Queue or Worker
pub use inventory::submit as register_component;

/// Helper type to iterate components
pub use inventory::iter as iterate_components;

impl<DT> ComponentRegistry<DT>
where
    DT: Clone + Send + Sync + 'static,
{
    pub fn register_components(
        mut storage: Storage,
        items: impl Iterator<Item = &'static Self>,
    ) -> Storage {
        for component in items {
            tracing::info!(
                "Registering {}::{}",
                component.module_path,
                component.type_name
            );
            match (component.definition)() {
                ComponentDefinition::Queue(q) => storage = storage.register_queue_with(q),
                ComponentDefinition::Worker(w) => storage = storage.register_worker_with(w),
            }
        }
        storage
    }
}
