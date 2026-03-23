//! Stdio JSON-RPC MCP server for Ristretto.

mod tools;

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Once;

use serde::Deserialize;
use serde_json::{json, Value};

use rist::daemon_client::DaemonClient;

const PROTOCOL_VERSION: &str = "2024-11-05";
static HOME_WARNING: Once = Once::new();

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

fn ristretto_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            HOME_WARNING.call_once(|| {
                eprintln!(
                    "warning: HOME is unset; falling back to current directory for .ristretto"
                );
            });
            PathBuf::from(".")
        })
        .join(".ristretto")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let socket_path = ristretto_dir().join("daemon.sock");
    let client = DaemonClient::connect(socket_path).await.ok();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut writer = io::BufWriter::new(stdout.lock());

    for line_result in stdin.lock().lines() {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => handle_request(request, client.as_ref()).await,
            Err(error) => Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {
                    "code": -32700,
                    "message": format!("parse error: {error}"),
                }
            })),
        };

        if let Some(response) = response {
            serde_json::to_writer(&mut writer, &response)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }

    Ok(())
}

async fn handle_request(request: JsonRpcRequest, client: Option<&DaemonClient>) -> Option<Value> {
    if request.jsonrpc != "2.0" {
        return Some(error_response(
            request.id.unwrap_or(Value::Null),
            -32600,
            "jsonrpc must be 2.0",
        ));
    }

    match request.method.as_str() {
        "notifications/initialized" => None,
        "initialize" => Some(success_response(
            request.id.unwrap_or(Value::Null),
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "ristretto",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        )),
        "tools/list" => Some(success_response(
            request.id.unwrap_or(Value::Null),
            json!({ "tools": tools::tool_definitions() }),
        )),
        "tools/call" => {
            let id = request.id.unwrap_or(Value::Null);
            let Some(client) = client else {
                return Some(error_response(id, -32000, "daemon not running"));
            };
            let name = request
                .params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match tools::handle_tool_call(client, name, arguments).await {
                Ok(result) => Some(success_response(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&result)
                                .unwrap_or_else(|_| "{}".to_owned()),
                        }],
                        "structuredContent": result,
                    }),
                )),
                Err(message) => Some(error_response(id, -32000, &message)),
            }
        }
        _ => Some(error_response(
            request.id.unwrap_or(Value::Null),
            -32601,
            "method not found",
        )),
    }
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn initialize_and_tools_list_handshake() {
        let initialize = handle_request(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "initialize".to_owned(),
                params: json!({}),
            },
            None,
        )
        .await
        .expect("initialize response");
        assert_eq!(
            initialize
                .get("result")
                .and_then(|value| value.get("protocolVersion"))
                .and_then(Value::as_str),
            Some(PROTOCOL_VERSION)
        );

        let tools = handle_request(
            JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(2)),
                method: "tools/list".to_owned(),
                params: json!({}),
            },
            None,
        )
        .await
        .expect("tools list response");
        assert_eq!(
            tools
                .get("result")
                .and_then(|value| value.get("tools"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(15)
        );
    }
}
