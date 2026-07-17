pub mod protocol;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::mcp::protocol::{JsonRpcRequest, JsonRpcResponse};

#[derive(Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

pub struct McpRegistry {
    servers: Vec<McpServerConfig>,
}

impl McpRegistry {
    pub fn load_default() -> Result<Self> {
        let path = default_config_path()?;
        Self::load(path)
    }

    fn load(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                servers: Vec::new(),
            });
        }

        let content = fs::read_to_string(&path).context("读取 mcp_servers.json 失败")?;
        let servers = serde_json::from_str(&content).context("解析 mcp_servers.json 失败")?;
        Ok(Self { servers })
    }

    pub fn list(&self) -> String {
        if self.servers.is_empty() {
            return format!(
                "还没有配置 MCP Server。\n配置文件位置：{}",
                default_config_path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| ".agent_data/mcp_servers.json".to_string())
            );
        }

        self.servers
            .iter()
            .map(|server| {
                let args = if server.args.is_empty() {
                    "".to_string()
                } else {
                    format!(" {}", server.args.join(" "))
                };
                format!("- {}：{}{}", server.name, server.command, args)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn list_tools(&self, server_name: &str) -> Result<String> {
        let server = self.server(server_name)?;
        let result = call_mcp_method(server, "tools/list", json!({}))?;
        Ok(serde_json::to_string_pretty(&result)?)
    }

    pub fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<String> {
        let server = self.server(server_name)?;
        let result = call_mcp_method(
            server,
            "tools/call",
            json!({
                "name": tool_name,
                "arguments": arguments
            }),
        )?;
        Ok(serde_json::to_string_pretty(&result)?)
    }

    fn server(&self, name: &str) -> Result<&McpServerConfig> {
        self.servers
            .iter()
            .find(|server| server.name == name)
            .ok_or_else(|| anyhow!("没有找到 MCP Server：{name}"))
    }
}

fn call_mcp_method(server: &McpServerConfig, method: &str, params: Value) -> Result<Value> {
    let mut child = Command::new(&server.command)
        .args(&server.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("启动 MCP Server 失败：{}", server.name))?;

    let mut stdin = child.stdin.take().context("打开 MCP stdin 失败")?;
    let stdout = child.stdout.take().context("打开 MCP stdout 失败")?;
    let mut reader = BufReader::new(stdout);

    write_request(
        &mut stdin,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
            method: "initialize".to_string(),
            params: json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "rust-deepseek-agent",
                    "version": "0.1.0"
                }
            }),
        },
    )?;
    read_response(&mut reader)?;

    write_request(
        &mut stdin,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: json!({}),
        },
    )?;

    write_request(
        &mut stdin,
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(2),
            method: method.to_string(),
            params,
        },
    )?;

    let response = read_response(&mut reader)?;
    let _ = child.kill();

    if let Some(error) = response.error {
        return Err(anyhow!("MCP 调用失败：{error}"));
    }

    Ok(response.result.unwrap_or(Value::Null))
}

fn write_request(stdin: &mut impl Write, request: JsonRpcRequest) -> Result<()> {
    let line = serde_json::to_string(&request)?;
    stdin.write_all(line.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

fn read_response(reader: &mut impl BufRead) -> Result<JsonRpcResponse> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    serde_json::from_str(line.trim()).context("解析 MCP 响应失败")
}

fn default_config_path() -> Result<PathBuf> {
    Ok(std::env::current_dir()?
        .join(".agent_data")
        .join("mcp_servers.json"))
}
