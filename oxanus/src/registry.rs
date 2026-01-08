use crate::{Config, QueueConfig, Storage, worker_registry::WorkerConfig};

pub struct ComponentRegistry<DT, ET> {
    /// `module_path!()`
    pub module_path: &'static str,
    /// `stringify!(MyStruct)`
    pub type_name: &'static str,
    pub definition: fn() -> ComponentDefinition<DT, ET>,
}

pub enum ComponentDefinition<DT, ET> {
    Queue(QueueConfig),
    Worker(WorkerConfig<DT, ET>),
}

/// Macro to create a component registry
pub use inventory::collect as create_component_registry;

/// Macro to register a Queue or Worker
pub use inventory::submit as register_component;

/// Helper type to iterate components
pub use inventory::iter as iterate_components;

impl<DT, ET> ComponentRegistry<DT, ET>
where
    DT: 'static,
    ET: 'static,
{
    pub fn build_config(
        storage: &Storage,
        items: impl Iterator<Item = &'static Self>,
    ) -> Config<DT, ET> {
        let mut config = Config::new(storage);
        for component in items {
            tracing::info!(
                "Registering {}::{}",
                component.module_path,
                component.type_name
            );
            match (component.definition)() {
                ComponentDefinition::Queue(q) => config.register_queue_with(q),
                ComponentDefinition::Worker(w) => config.register_worker_with(w),
            }
        }
        config
    }
}
