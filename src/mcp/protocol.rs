use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    pub params: Value,
}

#[derive(Deserialize)]
pub struct JsonRpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[allow(dead_code)]
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}
