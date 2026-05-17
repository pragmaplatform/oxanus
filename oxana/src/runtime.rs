use std::ops::Deref;

use tokio_util::sync::CancellationToken;

use crate::Storage;
use crate::config::{Config, RuntimeSettings};

pub(crate) struct Runtime<DT> {
    pub(crate) config: Config<DT>,
    pub(crate) settings: RuntimeSettings,
    pub(crate) storage: Storage,
    pub(crate) cancel_token: CancellationToken,
}

impl<DT> Runtime<DT> {
    pub(crate) fn new(storage: Storage, config: Config<DT>, settings: RuntimeSettings) -> Self {
        Self {
            config,
            settings,
            storage,
            cancel_token: CancellationToken::new(),
        }
    }
}

impl<DT> Deref for Runtime<DT> {
    type Target = Config<DT>;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}
