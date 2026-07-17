use anyhow::{Result, anyhow};

use crate::llm::{LlmClient, ToolDefinition};
use crate::message::Message;
use crate::tools::{self, Tool};

pub struct SubAgent {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub system_prompt: &'static str,
    pub tool_names: &'static [&'static str],
    pub max_turns: usize,
}

pub struct SubAgentRegistry {
    agents: Vec<SubAgent>,
}

impl SubAgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: vec![
                SubAgent {
                    name: "rust_teacher",
                    aliases: &["teacher"],
                    description: "负责讲解 Rust 概念和代码",
                    system_prompt: "你是 Rust 老师。请用新手能懂的方式解释，避免一次讲太多。需要理解项目文件时，可以先读取文件再回答。",
                    tool_names: &[
                        "repo_map",
                        "ls",
                        "read",
                        "read_lines",
                        "search_text",
                        "rag_search",
                        "web_search",
                        "web_fetch",
                    ],
                    max_turns: 8,
                },
                SubAgent {
                    name: "reviewer",
                    aliases: &["review"],
                    description: "负责审查代码风险和可维护性",
                    system_prompt: "你是代码审查员。请优先找 bug、边界问题、行为回归和结构问题。需要先读取相关文件，再给出具体结论。",
                    tool_names: &[
                        "repo_map",
                        "ls",
                        "read",
                        "read_lines",
                        "search_text",
                        "git_status",
                        "git_diff",
                        "run_command",
                        "validate_project",
                        "rag_search",
                    ],
                    max_turns: 10,
                },
                SubAgent {
                    name: "planner",
                    aliases: &["plan"],
                    description: "负责把目标拆成步骤",
                    system_prompt: "你是任务规划代理。请先理解目标和现有上下文，再把目标拆成可执行步骤，并说明先做什么。",
                    tool_names: &[
                        "repo_map",
                        "ls",
                        "read",
                        "read_lines",
                        "search_text",
                        "git_status",
                        "git_diff",
                        "rag_search",
                    ],
                    max_turns: 8,
                },
                SubAgent {
                    name: "researcher",
                    aliases: &["research"],
                    description: "负责研究网页、外部资料和背景信息，并整理结论",
                    system_prompt: "你是研究代理。请先澄清已知信息，再使用可用检索工具收集证据，最后给出结构化结论和来源线索。",
                    tool_names: &[
                        "repo_map",
                        "search_text",
                        "web_search",
                        "web_fetch",
                        "rag_search",
                        "read",
                        "read_lines",
                    ],
                    max_turns: 12,
                },
            ],
        }
    }

    pub fn list(&self) -> String {
        self.agents
            .iter()
            .map(|agent| {
                format!(
                    "- {}：{}；工具：{}；最多 {} 轮",
                    agent.name,
                    agent.description,
                    agent.tool_names.join(", "),
                    agent.max_turns
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn prompt_summary(&self) -> String {
        self.agents
            .iter()
            .map(|agent| {
                let aliases = if agent.aliases.is_empty() {
                    String::new()
                } else {
                    format!("，别名：{}", agent.aliases.join(", "))
                };
                format!(
                    "- {}{}：{}。可用工具：{}；最多 {} 轮。",
                    agent.name,
                    aliases,
                    agent.description,
                    agent.tool_names.join(", "),
                    agent.max_turns
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub async fn run(
        &self,
        name: &str,
        llm: &LlmClient,
        task: &str,
        tools: &[Tool],
    ) -> Result<String> {
        let agent = self
            .resolve(name)
            .ok_or_else(|| anyhow!("没有找到子代理：{name}"))?;

        let allowed_tools: Vec<&Tool> = tools
            .iter()
            .filter(|tool| agent.tool_names.contains(&tool.name))
            .collect();

        let tool_definitions: Vec<ToolDefinition> = allowed_tools
            .iter()
            .map(|tool| {
                ToolDefinition::function(
                    tool.name,
                    format!("{}。用法参考：{}", tool.description, tool.usage),
                    tool.parameters.clone(),
                )
            })
            .collect();

        let mut messages = vec![
            Message::new("system", sub_agent_system_prompt(agent)),
            Message::new("user", task),
        ];

        if tool_definitions.is_empty() {
            return llm.ask(&messages).await;
        }

        for _ in 0..agent.max_turns {
            let turn = llm
                .ask_with_tools(&messages, tool_definitions.clone())
                .await?;

            if turn.tool_calls.is_empty() {
                return Ok(turn.content.unwrap_or_default());
            }

            let assistant_content = turn
                .content
                .clone()
                .filter(|content| !content.trim().is_empty());
            messages.push(Message::assistant_tool_calls(
                assistant_content,
                turn.tool_calls.clone(),
            ));

            for tool_call in turn.tool_calls {
                let tool_name = tool_call.function.name.clone();
                let output = match allowed_tools
                    .iter()
                    .copied()
                    .find(|tool| tool.name == tool_name)
                {
                    Some(tool) => {
                        match tools::tool_arguments_to_input(tool, &tool_call.function.arguments) {
                            Ok(input) => {
                                match tools::execute_tool(llm.http_client(), &tool_name, &input)
                                    .await
                                {
                                    Ok(output) => output,
                                    Err(error) => format!("工具 {tool_name} 执行失败：{error}"),
                                }
                            }
                            Err(error) => format!("工具 {tool_name} 参数错误：{error}"),
                        }
                    }
                    None => format!("子代理不允许调用工具：{tool_name}"),
                };

                messages.push(Message::tool_result(tool_call.id, output));
            }
        }

        Ok(format!(
            "子代理 `{}` 已达到 max_turns={} 上限，已停止以避免循环。请让主 Agent 根据已有结果继续判断。",
            agent.name, agent.max_turns
        ))
    }

    fn resolve(&self, name: &str) -> Option<&SubAgent> {
        self.agents
            .iter()
            .find(|agent| agent.name == name || agent.aliases.contains(&name))
    }
}

fn sub_agent_system_prompt(agent: &SubAgent) -> String {
    format!(
        "{}\n\
你是主 Agent 派遣出来的独立子代理。\n\
你的任务是完成用户给你的子任务，然后把结论返回给主 Agent。\n\
你有独立上下文，不要假设主 Agent 看得到你的中间过程。\n\
如果需要工具，请自主调用允许的工具；如果不需要工具，直接回答。\n\
最终回答要包含：你做了什么、关键发现、建议主 Agent 下一步怎么做。\n\
不要修改主 Agent 的 Memory 或 Todo；这些由主 Agent 决定。\n\
允许工具：{}。",
        agent.system_prompt,
        agent.tool_names.join(", ")
    )
}
