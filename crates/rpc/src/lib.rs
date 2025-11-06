// rpc/src/lib.rs
pub mod server;
pub mod methods;
pub mod types;

pub use server::{RpcServer, RpcConfig};
pub use methods::RpcMethods;
pub use types::*;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("Parse error")]
    ParseError,
    #[error("Invalid request")]
    InvalidRequest,
    #[error("Method not found: {0}")]
    MethodNotFound(String),
    #[error("Invalid params: {0}")]
    InvalidParams(String),
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("Server error: {0}")]
    ServerError(String),
}

impl RpcError {
    pub fn code(&self) -> i32 {
        match self {
            RpcError::ParseError => -32700,
            RpcError::InvalidRequest => -32600,
            RpcError::MethodNotFound(_) => -32601,
            RpcError::InvalidParams(_) => -32602,
            RpcError::InternalError(_) => -32603,
            RpcError::ServerError(_) => -32000,
        }
    }
}

pub type RpcResult<T> = Result<T, RpcError>;
