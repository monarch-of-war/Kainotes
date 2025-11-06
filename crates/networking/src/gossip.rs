// networking/src/gossip.rs
use blockchain_core::{Block, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GossipTopic {
    NewBlocks,
    NewTransactions,
    Consensus,
}

pub struct GossipService {
    topics: Vec<GossipTopic>,
}

impl GossipService {
    pub fn new() -> Self {
        Self {
            topics: vec![
                GossipTopic::NewBlocks,
                GossipTopic::NewTransactions,
                GossipTopic::Consensus,
            ],
        }
    }

    pub fn broadcast_block(&self, _block: &Block) {
        tracing::debug!("Broadcasting block");
    }

    pub fn broadcast_transaction(&self, _tx: &Transaction) {
        tracing::debug!("Broadcasting transaction");
    }
}

impl Default for GossipService {
    fn default() -> Self {
        Self::new()
    }
}