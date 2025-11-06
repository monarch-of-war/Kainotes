// networking/src/sync.rs
use blockchain_core::BlockNumber;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    Idle,
    Syncing { current: BlockNumber, target: BlockNumber },
    Complete,
}

#[derive(Debug, Clone, Copy)]
pub enum SyncStrategy {
    FastSync,
    FullSync,
}

pub struct SyncManager {
    status: SyncStatus,
    strategy: SyncStrategy,
}

impl SyncManager {
    pub fn new(strategy: SyncStrategy) -> Self {
        Self {
            status: SyncStatus::Idle,
            strategy,
        }
    }

    pub fn status(&self) -> SyncStatus {
        self.status
    }

    pub fn is_syncing(&self) -> bool {
        matches!(self.status, SyncStatus::Syncing { .. })
    }
}