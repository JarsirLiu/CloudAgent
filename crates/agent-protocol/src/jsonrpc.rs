use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Integer(i64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    Response(JsonRpcResponse),
    Error(JsonRpcError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub id: RequestId,
    pub result: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub id: RequestId,
    pub error: JsonRpcErrorPayload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcErrorPayload {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
