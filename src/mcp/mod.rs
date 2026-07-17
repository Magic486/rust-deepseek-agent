use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rmcp::model::{CallToolRequestParams, ContentBlock};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MCP_CALL_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub environment: HashMap<String, String>,
}

#[derive(Clone)]
pub struct McpToolDescriptor {
    pub qualified_name: String,
    pub server_name: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone)]
pub struct McpServerSnapshot {
    pub name: String,
    pub connected: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

struct McpConnection {
    client: RunningService<RoleClient, ()>,
    tools: Vec<McpToolDescriptor>,
}

struct McpServerState {
    config: McpServerConfig,
    connection: Option<McpConnection>,
    error: Option<String>,
}

pub struct McpRegistry {
    servers: Vec<McpServerState>,
}

impl McpRegistry {
    pub async fn load_default() -> Result<Self> {
        let path = default_config_path()?;
        let configs = load_configs(&path)?;
        let mut servers = Vec::new();

        for config in configs {
            if !config.enabled {
                servers.push(McpServerState {
                    config,
                    connection: None,
                    error: Some("已禁用".to_string()),
                });
                continue;
            }

            match connect_server(&config).await {
                Ok(connection) => servers.push(McpServerState {
                    config,
                    connection: Some(connection),
                    error: None,
                }),
                Err(error) => servers.push(McpServerState {
                    config,
                    connection: None,
                    error: Some(error.to_string()),
                }),
            }
        }

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
            .map(|server| match &server.connection {
                Some(connection) => format!(
                    "- {}：已连接，发现 {} 个工具",
                    server.config.name,
                    connection.tools.len()
                ),
                None => format!(
                    "- {}：未连接{}",
                    server.config.name,
                    server
                        .error
                        .as_deref()
                        .map(|error| format!("，{error}"))
                        .unwrap_or_default()
                ),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn snapshots(&self) -> Vec<McpServerSnapshot> {
        self.servers
            .iter()
            .map(|server| McpServerSnapshot {
                name: server.config.name.clone(),
                connected: server.connection.is_some(),
                tool_count: server
                    .connection
                    .as_ref()
                    .map(|connection| connection.tools.len())
                    .unwrap_or_default(),
                error: server.error.clone(),
            })
            .collect()
    }

    pub fn tools(&self) -> Vec<McpToolDescriptor> {
        self.servers
            .iter()
            .filter_map(|server| server.connection.as_ref())
            .flat_map(|connection| connection.tools.clone())
            .collect()
    }

    pub fn list_tools(&self, server_name: &str) -> Result<String> {
        let server = self.server(server_name)?;
        let Some(connection) = &server.connection else {
            return Err(anyhow!(
                "MCP Server `{server_name}` 当前未连接：{}",
                server.error.as_deref().unwrap_or("未知错误")
            ));
        };
        let lines = connection
            .tools
            .iter()
            .map(|tool| format!("- {}：{}", tool.tool_name, tool.description))
            .collect::<Vec<_>>();
        Ok(if lines.is_empty() {
            "该 MCP Server 没有提供工具。".to_string()
        } else {
            lines.join("\n")
        })
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<String> {
        let server = self.server(server_name)?;
        let connection = server
            .connection
            .as_ref()
            .ok_or_else(|| anyhow!("MCP Server `{server_name}` 未连接"))?;
        let arguments = arguments
            .as_object()
            .cloned()
            .context("MCP 工具参数必须是 JSON 对象")?;
        let request = CallToolRequestParams::new(tool_name.to_string()).with_arguments(arguments);
        let result = tokio::time::timeout(MCP_CALL_TIMEOUT, connection.client.call_tool(request))
            .await
            .context("MCP 工具调用超时")??;

        let mut output = result
            .content
            .iter()
            .filter_map(|content| match content {
                ContentBlock::Text(text) => Some(text.text.clone()),
                _ => serde_json::to_string(content).ok(),
            })
            .collect::<Vec<_>>();
        if let Some(structured) = result.structured_content {
            output.push(serde_json::to_string_pretty(&structured)?);
        }
        if result.is_error == Some(true) {
            return Err(anyhow!("MCP 工具返回错误：{}", output.join("\n")));
        }
        Ok(if output.is_empty() {
            "MCP 工具调用完成，但没有返回内容。".to_string()
        } else {
            output.join("\n")
        })
    }

    pub async fn call_qualified_tool(
        &self,
        qualified_name: &str,
        arguments: Value,
    ) -> Result<String> {
        let descriptor = self
            .tools()
            .into_iter()
            .find(|tool| tool.qualified_name == qualified_name)
            .ok_or_else(|| anyhow!("没有找到 MCP 工具：{qualified_name}"))?;
        self.call_tool(&descriptor.server_name, &descriptor.tool_name, arguments)
            .await
    }

    fn server(&self, name: &str) -> Result<&McpServerState> {
        self.servers
            .iter()
            .find(|server| server.config.name == name)
            .ok_or_else(|| anyhow!("没有找到 MCP Server：{name}"))
    }
}

async fn connect_server(config: &McpServerConfig) -> Result<McpConnection> {
    let mut command = tokio::process::Command::new(&config.command);
    command.args(&config.args);
    command.envs(&config.environment);
    let transport = TokioChildProcess::new(command.configure(|command| {
        command.kill_on_drop(true);
    }))
    .context("创建 MCP stdio transport 失败")?;
    let client = tokio::time::timeout(MCP_CONNECT_TIMEOUT, ().serve(transport))
        .await
        .context("MCP Server 初始化超时")??;
    let tools = tokio::time::timeout(MCP_CONNECT_TIMEOUT, client.list_all_tools())
        .await
        .context("读取 MCP 工具列表超时")??;
    let tools = tools
        .into_iter()
        .map(|tool| -> Result<McpToolDescriptor> {
            let tool_name = tool.name.to_string();
            Ok(McpToolDescriptor {
                qualified_name: qualified_tool_name(&config.name, &tool_name),
                server_name: config.name.clone(),
                tool_name,
                description: tool
                    .description
                    .map(|description| description.to_string())
                    .unwrap_or_else(|| "MCP 外部工具".to_string()),
                input_schema: serde_json::to_value(tool.input_schema.as_ref())?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(McpConnection { client, tools })
}

pub fn qualified_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", sanitize_name(server), sanitize_name(tool))
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn load_configs(path: &PathBuf) -> Result<Vec<McpServerConfig>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).context("读取 mcp_servers.json 失败")?;
    serde_json::from_str(&content).context("解析 mcp_servers.json 失败")
}

fn default_enabled() -> bool {
    true
}

fn default_config_path() -> Result<PathBuf> {
    Ok(std::env::current_dir()?
        .join(".agent_data")
        .join("mcp_servers.json"))
}

#[cfg(test)]
mod tests {
    use super::qualified_tool_name;

    #[test]
    fn qualifies_mcp_tool_for_llm() {
        assert_eq!(
            qualified_tool_name("filesystem server", "read-file"),
            "mcp__filesystem_server__read-file"
        );
    }
}
