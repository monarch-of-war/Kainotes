// networking/src/protocol.rs

use blockchain_core::{Block, BlockNumber, Transaction};
use blockchain_crypto::Hash;
use serde::{Deserialize, Serialize};

/// Protocol message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolMessage {
    /// Handshake message
    Handshake(HandshakeMessage),
    /// Status update
    Status(StatusMessage),
    /// Request blocks
    GetBlocks(GetBlocksMessage),
    /// Send blocks
    Blocks(BlocksMessage),
    /// Request transactions
    GetTransactions(GetTransactionsMessage),
    /// Send transactions
    Transactions(TransactionsMessage),
    /// New block announcement
    NewBlock(NewBlockMessage),
    /// New transaction announcement
    NewTransaction(NewTransactionMessage),
    /// Ping/Pong for keepalive
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeMessage {
    pub protocol_version: u32,
    pub client_version: String,
    pub network_id: u64,
    pub best_block: BlockNumber,
    pub genesis_hash: Hash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusMessage {
    pub best_block: BlockNumber,
    pub best_block_hash: Hash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetBlocksMessage {
    pub start_block: BlockNumber,
    pub max_blocks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlocksMessage {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTransactionsMessage {
    pub hashes: Vec<Hash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionsMessage {
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBlockMessage {
    pub block: Block,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTransactionMessage {
    pub transaction: Transaction,
}

/// Message type for gossip protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageType {
    Block,
    Transaction,
    Status,
}