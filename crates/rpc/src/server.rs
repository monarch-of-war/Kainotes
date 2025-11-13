// rpc/src/server.rs
use crate::{RpcError, RpcResult, RpcRequest, RpcResponse, RpcErrorResponse, RpcMethods};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use hyper::{Body, Request, Response, Server, StatusCode, Method};
use hyper::service::{make_service_fn, service_fn};

#[derive(Debug, Clone)]
pub struct RpcConfig {
    pub listen_addr: SocketAddr,
    pub max_connections: usize,
    pub enable_ws: bool,
    pub cors_origins: Vec<String>,
    // New configuration options
    pub enable_mempool_methods: bool,
    pub max_range_query_blocks: u64,
    pub metrics_cache_duration: u64, // seconds
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:8545".parse().unwrap(),
            max_connections: 100,
            enable_ws: false,
            cors_origins: vec!["*".to_string()],
            enable_mempool_methods: true,
            max_range_query_blocks: 100,
            metrics_cache_duration: 1,
        }
    }
}

pub struct RpcServer {
    config: RpcConfig,
    methods: Arc<RwLock<RpcMethods>>,
}

impl RpcServer {
    pub fn new(config: RpcConfig, methods: RpcMethods) -> Self {
        Self {
            config,
            methods: Arc::new(RwLock::new(methods)),
        }
    }

    pub async fn start(self: Arc<Self>) -> RpcResult<()> {
        tracing::info!("Starting RPC server on {}", self.config.listen_addr);

        let value = self.clone();
        let make_svc = make_service_fn(move |_| {
            let server = value.clone();
            async move {
                Ok::<_, hyper::Error>(service_fn(move |req| {
                    let server = server.clone();
                    async move { server.handle_request(req).await }
                }))
            }
        });

        let server = Server::bind(&self.config.listen_addr)
            .serve(make_svc);

        tracing::info!("RPC server listening on {}", self.config.listen_addr);

        server.await
            .map_err(|e| RpcError::ServerError(e.to_string()))?;

        Ok(())
    }

    async fn handle_request(&self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        // CORS headers
        let mut response_builder = Response::builder()
            .header("Content-Type", "application/json")
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "POST, OPTIONS")
            .header("Access-Control-Allow-Headers", "Content-Type");

        // Handle OPTIONS
        if req.method() == Method::OPTIONS {
            return Ok(response_builder.status(StatusCode::OK).body(Body::empty()).unwrap());
        }

        // Only accept POST
        if req.method() != Method::POST {
            return Ok(response_builder.status(StatusCode::METHOD_NOT_ALLOWED).body(Body::from("Method not allowed")).unwrap());
        }

        // Read body
        let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
        
        let rpc_request: RpcRequest = match serde_json::from_slice(&body_bytes) {
            Ok(req) => req,
            Err(_) => {
                let error_response = RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(RpcErrorResponse {
                        code: -32700,
                        message: "Parse error".to_string(),
                        data: None,
                    }),
                    id: serde_json::Value::Null,
                };
                let json = serde_json::to_string(&error_response).unwrap();
                return Ok(response_builder.status(StatusCode::OK).body(Body::from(json)).unwrap());
            }
        };

        // Process request
        let response = self.process_request(rpc_request).await;
        let json = serde_json::to_string(&response).unwrap();

        Ok(response_builder.status(StatusCode::OK).body(Body::from(json)).unwrap())
    }

    async fn process_request(&self, request: RpcRequest) -> RpcResponse {
        let methods = self.methods.read().await;
        
        match methods.handle(&request.method, request.params.clone()).await {
            Ok(result) => RpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(result),
                error: None,
                id: request.id,
            },
            Err(error) => RpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(RpcErrorResponse {
                    code: error.code(),
                    message: error.to_string(),
                    data: None,
                }),
                id: request.id,
            },
        }
    }
}