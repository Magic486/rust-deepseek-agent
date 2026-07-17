use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message::{Message, ToolCall};

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    tool_type: String,
    function: ToolFunctionDefinition,
}

#[derive(Serialize, Clone)]
struct ToolFunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
    finish_reason: Option<String>,
}

pub struct LlmTurn {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
}

#[derive(Clone)]
pub struct LlmClient {
    api_key: String,
    http_client: Client,
}

impl LlmClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http_client: Client::new(),
        }
    }

    pub fn http_client(&self) -> &Client {
        &self.http_client
    }

    pub async fn ask(&self, messages: &[Message]) -> Result<String> {
        let request_body = ChatRequest {
            model: DEFAULT_MODEL.to_string(),
            messages: messages.to_vec(),
            tools: None,
            tool_choice: None,
        };

        let turn = self.send_chat_request(request_body).await?;
        let answer = turn.content.context("DeepSeek API 响应里没有回答内容")?;

        Ok(answer)
    }

    pub async fn ask_with_tools(
        &self,
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<LlmTurn> {
        let request_body = ChatRequest {
            model: DEFAULT_MODEL.to_string(),
            messages: messages.to_vec(),
            tools: Some(tools),
            tool_choice: Some("auto".to_string()),
        };

        self.send_chat_request(request_body).await
    }

    pub async fn ask_with_system(&self, system_prompt: &str, user_input: &str) -> Result<String> {
        let messages = vec![
            Message::new("system", system_prompt),
            Message::new("user", user_input),
        ];

        self.ask(&messages).await
    }

    async fn send_chat_request(&self, request_body: ChatRequest) -> Result<LlmTurn> {
        let response = self
            .http_client
            .post(DEEPSEEK_API_URL)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .await
            .context("请求 DeepSeek API 失败")?
            .error_for_status()
            .context("DeepSeek API 返回了错误状态码")?
            .json::<ChatResponse>()
            .await
            .context("解析 DeepSeek API 响应失败")?;

        let choice = response
            .choices
            .into_iter()
            .next()
            .context("DeepSeek API 响应里没有 choices")?;
        let tool_calls = choice.message.tool_calls.clone().unwrap_or_default();

        Ok(LlmTurn {
            content: choice.message.content,
            tool_calls,
            finish_reason: choice.finish_reason,
        })
    }
}

impl ToolDefinition {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
    ) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: ToolFunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}
