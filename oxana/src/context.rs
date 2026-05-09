use crate::{job_envelope::JobMeta, job_state::JobState};

#[derive(Clone)]
pub struct JobContext {
    pub meta: JobMeta,
    pub state: JobState,
}

#[derive(Debug, Clone)]
pub struct ContextValue<T: Clone + Send + Sync>(pub(crate) T);

impl<T: Clone + Send + Sync> ContextValue<T> {
    pub fn new(v: T) -> Self {
        Self(v)
    }
}
