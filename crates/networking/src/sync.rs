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

    /// Called by networking layer when a fork is detected to coordinate resolution
    pub fn handle_fork_notification(&mut self) {
        // Placeholder: in a full implementation this would trigger chain segment
        // requests and fork resolution using a ForkResolver instance
        tracing::info!("SyncManager: fork notification received");
    }

    /// Trigger a mempool sync after catching up
    pub fn trigger_mempool_sync(&mut self) {
        tracing::info!("SyncManager: triggering mempool sync");
    }
}