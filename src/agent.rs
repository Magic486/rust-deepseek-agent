use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::hooks;
use crate::llm::{LlmClient, ToolDefinition};
use crate::mcp::McpRegistry;
use crate::memory::MemoryStore;
use crate::message::Message;
use crate::retrieval::RetrievalIndex;
use crate::skills::{SkillRegistry, SkillSnapshotItem};
use crate::sub_agent::SubAgentRegistry;
use crate::todo::{TodoList, TodoStatus, status_label};
use crate::tools::{self, Tool, ToolSource};
use crate::workspace::WorkspaceContext;

const MAX_AGENT_STEPS: usize = 16;
const MAX_REPEATED_TOOL_CALLS: usize = 2;
const COMPACT_HISTORY_CHARS: usize = 120_000;
const KEEP_RECENT_MESSAGES: usize = 32;

#[derive(Clone)]
pub enum AgentStatus {
    Ready,
    Thinking,
    RunningTool(String),
    Error(String),
}

#[derive(Clone)]
pub enum AgentEvent {
    UserMessage(String),
    AssistantMessage(String),
    ToolCall {
        name: String,
        input: String,
    },
    ToolResult {
        name: String,
        output: String,
    },
    ToolError {
        name: String,
        error: String,
    },
    SystemMessage(String),
    TodoUpdated,
    RuntimeSnapshot {
        todos: Vec<TodoSnapshotItem>,
        skills: Vec<SkillSnapshotItem>,
        mcp_servers: Vec<crate::mcp::McpServerSnapshot>,
    },
    StatusChanged(AgentStatus),
}

pub struct AgentTurnResult {
    pub events: Vec<AgentEvent>,
    pub should_exit: bool,
}

#[derive(Clone)]
pub struct TodoSnapshotItem {
    pub id: usize,
    pub title: String,
    pub status: String,
}

impl AgentTurnResult {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            should_exit: false,
        }
    }
}

#[derive(Deserialize)]
struct MemoryAddInput {
    #[serde(default = "default_memory_kind")]
    kind: String,
    content: String,
}

#[derive(Deserialize)]
struct TodoAddInput {
    #[serde(default)]
    title: String,
    #[serde(default)]
    titles: Vec<String>,
}

#[derive(Deserialize)]
struct TodoDoneInput {
    id: usize,
}

#[derive(Deserialize)]
struct DispatchSubAgentInput {
    agent_type: String,
    task: String,
    #[serde(default)]
    purpose: Option<String>,
}

#[derive(Deserialize)]
struct SkillLoadInput {
    name: String,
}

pub struct Agent {
    llm: LlmClient,
    tools: Vec<Tool>,
    messages: Vec<Message>,
    memory: MemoryStore,
    retrieval: RetrievalIndex,
    mcp: McpRegistry,
    todo: TodoList,
    skills: SkillRegistry,
    sub_agents: SubAgentRegistry,
    workspace: WorkspaceContext,
}

impl Agent {
    pub async fn new(api_key: String) -> Result<Self> {
        let tools = tools::registered_tools();
        let memory = MemoryStore::load_default()?;
        let retrieval = RetrievalIndex::load_default()?;
        let mcp = McpRegistry::load_default().await?;
        let workspace = WorkspaceContext::load()?;
        let todo = TodoList::new_session();
        let skills = SkillRegistry::discover(&workspace.root)?;
        let sub_agents = SubAgentRegistry::new();
        let messages = vec![Message::new(
            "system",
            build_agent_system_prompt(&tools, &skills, &sub_agents, &workspace, &mcp),
        )];

        Ok(Self {
            llm: LlmClient::new(api_key),
            tools,
            messages,
            memory,
            retrieval,
            mcp,
            todo,
            skills,
            sub_agents,
            workspace,
        })
    }

    pub async fn handle_user_input(&mut self, user_input: &str) -> Result<AgentTurnResult> {
        self.handle_user_input_stream(user_input, |_| Ok(())).await
    }

    pub async fn handle_user_input_stream<F>(
        &mut self,
        user_input: &str,
        mut observer: F,
    ) -> Result<AgentTurnResult>
    where
        F: FnMut(&AgentEvent) -> Result<()>,
    {
        if user_input.eq_ignore_ascii_case("exit") || user_input == "/quit" {
            let mut result = AgentTurnResult::new();
            emit_event(
                &mut result.events,
                &mut observer,
                AgentEvent::SystemMessage("下次继续。".to_string()),
            )?;
            result.should_exit = true;
            return Ok(result);
        }

        if user_input.is_empty() {
            let mut result = AgentTurnResult::new();
            emit_event(
                &mut result.events,
                &mut observer,
                AgentEvent::SystemMessage("问题不能为空。".to_string()),
            )?;
            return Ok(result);
        }

        if let Some(events) = self.handle_system_command(user_input).await? {
            return observed_result(events, &mut observer);
        }

        if let Some(events) = self
            .handle_manual_tool_command(user_input, &mut observer)
            .await?
        {
            return Ok(AgentTurnResult {
                events,
                should_exit: false,
            });
        }

        let mut result = AgentTurnResult::new();
        emit_event(
            &mut result.events,
            &mut observer,
            AgentEvent::UserMessage(user_input.to_string()),
        )?;
        emit_event(
            &mut result.events,
            &mut observer,
            AgentEvent::StatusChanged(AgentStatus::Thinking),
        )?;

        let user_message = hooks::before_llm_user_message(&self.memory, user_input);
        let user_message = self.enrich_with_todos(&user_message);
        self.messages.push(Message::new("user", user_message));
        if self.compact_history_if_needed() {
            emit_event(
                &mut result.events,
                &mut observer,
                AgentEvent::SystemMessage(
                    "对话上下文较长，已保留系统规则和最近消息，较早内容已压缩。".to_string(),
                ),
            )?;
        }
        self.run_agent_turn(&mut result.events, &mut observer)
            .await?;
        emit_event(
            &mut result.events,
            &mut observer,
            AgentEvent::StatusChanged(AgentStatus::Ready),
        )?;

        Ok(result)
    }

    pub fn todo_snapshot(&self) -> Vec<TodoSnapshotItem> {
        self.todo
            .items()
            .iter()
            .map(|item| TodoSnapshotItem {
                id: item.id,
                title: item.title.clone(),
                status: status_label(&item.status).to_string(),
            })
            .collect()
    }

    pub fn skill_snapshot(&self) -> Vec<SkillSnapshotItem> {
        self.skills.snapshot()
    }

    pub fn mcp_snapshot(&self) -> Vec<crate::mcp::McpServerSnapshot> {
        self.mcp.snapshots()
    }

    pub fn local_tool_count(&self) -> usize {
        self.tools.len()
    }

    pub fn runtime_snapshot(&self) -> AgentEvent {
        AgentEvent::RuntimeSnapshot {
            todos: self.todo_snapshot(),
            skills: self.skill_snapshot(),
            mcp_servers: self.mcp_snapshot(),
        }
    }

    pub fn reset_session(&mut self) {
        self.todo.clear();
        self.skills.reset_loaded();
        self.messages = vec![Message::new(
            "system",
            build_agent_system_prompt(
                &self.tools,
                &self.skills,
                &self.sub_agents,
                &self.workspace,
                &self.mcp,
            ),
        )];
    }

    fn enrich_with_todos(&self, user_message: &str) -> String {
        let active: Vec<String> = self
            .todo
            .items()
            .iter()
            .filter(|item| matches!(item.status, TodoStatus::Pending | TodoStatus::InProgress))
            .take(8)
            .map(|item| {
                format!(
                    "- #{} [{}] {}",
                    item.id,
                    status_label(&item.status),
                    item.title
                )
            })
            .collect();

        if active.is_empty() {
            return user_message.to_string();
        }

        format!(
            "{user_message}\n\n当前会话 Todo：\n{}\n\n执行规则：如果用户请求和 Todo 相关，请优先推进 Todo；开始执行某一步时用 todo_update 标记 in_progress；完成后用 todo_update 或 todo_done 标记 done；遇到阻塞时用 todo_update 标记 blocked。",
            active.join("\n")
        )
    }

    async fn handle_system_command(&mut self, user_input: &str) -> Result<Option<Vec<AgentEvent>>> {
        if matches!(user_input, "/clear" | "/new") {
            self.reset_session();
            return Ok(Some(vec![
                AgentEvent::SystemMessage("已开始新会话，对话上下文和 Todo 已清空。".to_string()),
                AgentEvent::TodoUpdated,
            ]));
        }

        if user_input == "/help" {
            return Ok(Some(vec![AgentEvent::SystemMessage(
                "系统命令：\n\
/help                  查看帮助\n\
/memory add 内容       保存长期记忆\n\
/memory list           查看长期记忆\n\
/memory search 关键词  搜索长期记忆\n\
/todo add 任务         添加待办\n\
/todo list             查看待办\n\
/todo done 编号        完成待办\n\
/plan 目标             让 AI 拆解目标并写入待办\n\
/rag sources           查看 RAG 外部数据源\n\
/rag add-folder 名称 路径  添加外部文件夹数据源\n\
/rag remove 编号       删除 RAG 数据源\n\
/rag reindex           重建 RAG 索引\n\
/rag search 关键词     检索外部数据源\n\
/mcp list              查看 MCP Server 配置\n\
/mcp tools 名称        查看某个 MCP Server 的工具\n\
/mcp call 名称 工具 JSON  调用 MCP 工具\n\
/skills                查看技能\n\
/skill use 名称        启用技能\n\
/subagents             查看子代理\n\
/subagent 名称 问题    让子代理处理问题\n\
/new 或 /clear         开始新会话并清空 Todo"
                    .to_string(),
            )]));
        }

        if let Some(input) = user_input.strip_prefix("/memory ") {
            return Ok(Some(vec![self.handle_memory_command(input)?]));
        }

        if let Some(input) = user_input.strip_prefix("/todo ") {
            return Ok(Some(self.handle_todo_command(input)?));
        }

        if let Some(goal) = user_input.strip_prefix("/plan ") {
            return Ok(Some(self.plan_todos(goal).await?));
        }

        if let Some(input) = user_input.strip_prefix("/rag ") {
            return Ok(Some(vec![self.handle_rag_command(input)?]));
        }

        if let Some(input) = user_input.strip_prefix("/mcp ") {
            return Ok(Some(vec![self.handle_mcp_command(input).await?]));
        }

        if user_input == "/skills" {
            return Ok(Some(vec![AgentEvent::SystemMessage(self.skills.list())]));
        }

        if let Some(skill_name) = user_input.strip_prefix("/skill use ") {
            let message = match self.skills.load(skill_name.trim()) {
                Ok(message) => {
                    self.refresh_system_prompt();
                    message
                }
                Err(error) => format!("技能出错：{error}"),
            };
            return Ok(Some(vec![AgentEvent::SystemMessage(message)]));
        }

        if user_input == "/subagents" {
            return Ok(Some(vec![AgentEvent::SystemMessage(
                self.sub_agents.list(),
            )]));
        }

        if let Some(input) = user_input.strip_prefix("/subagent ") {
            return Ok(Some(vec![self.run_sub_agent(input).await?]));
        }

        Ok(None)
    }

    async fn handle_manual_tool_command(
        &mut self,
        user_input: &str,
        observer: &mut impl FnMut(&AgentEvent) -> Result<()>,
    ) -> Result<Option<Vec<AgentEvent>>> {
        if !user_input.starts_with('/') {
            return Ok(None);
        }

        if user_input == "/tools" {
            return Ok(Some(vec![AgentEvent::SystemMessage(format_tool_list(
                &self.tools,
            ))]));
        }

        let command = &user_input[1..];
        let mut parts = command.splitn(2, ' ');
        let tool_name = parts.next().unwrap_or("");
        let tool_input = parts.next().unwrap_or("").trim();

        if !self.tools.iter().any(|tool| tool.name == tool_name) {
            return Ok(None);
        }

        let mut events = Vec::new();
        emit_event(
            &mut events,
            observer,
            AgentEvent::ToolCall {
                name: tool_name.to_string(),
                input: tool_input.to_string(),
            },
        )?;
        emit_event(
            &mut events,
            observer,
            AgentEvent::StatusChanged(AgentStatus::RunningTool(tool_name.to_string())),
        )?;

        match self.execute_agent_tool(tool_name, tool_input).await {
            Ok(tool_output) => {
                hooks::after_tool_result(&mut self.messages, tool_name, tool_input, &tool_output);
                emit_event(
                    &mut events,
                    observer,
                    AgentEvent::ToolResult {
                        name: tool_name.to_string(),
                        output: tool_output,
                    },
                )?;
                if is_todo_tool(tool_name) {
                    emit_event(&mut events, observer, AgentEvent::TodoUpdated)?;
                }
                emit_event(
                    &mut events,
                    observer,
                    AgentEvent::StatusChanged(AgentStatus::Ready),
                )?;
            }
            Err(error) => {
                let error_message = error.to_string();
                emit_event(
                    &mut events,
                    observer,
                    AgentEvent::ToolError {
                        name: tool_name.to_string(),
                        error: error_message.clone(),
                    },
                )?;
                emit_event(
                    &mut events,
                    observer,
                    AgentEvent::StatusChanged(AgentStatus::Error(error_message)),
                )?;
            }
        }

        Ok(Some(events))
    }

    fn handle_memory_command(&mut self, input: &str) -> Result<AgentEvent> {
        if let Some(content) = input.strip_prefix("add ") {
            let item = self.memory.add("fact", content.trim())?;
            return Ok(AgentEvent::SystemMessage(format!(
                "已保存记忆 #{}：{}",
                item.id, item.content
            )));
        }

        if input == "list" {
            return Ok(AgentEvent::SystemMessage(self.memory.list()));
        }

        if let Some(keyword) = input.strip_prefix("search ") {
            return Ok(AgentEvent::SystemMessage(
                self.memory.search(keyword.trim()),
            ));
        }

        Ok(AgentEvent::SystemMessage(
            "用法：/memory add 内容，/memory list，/memory search 关键词".to_string(),
        ))
    }

    async fn handle_mcp_command(&self, input: &str) -> Result<AgentEvent> {
        if input == "list" {
            return Ok(AgentEvent::SystemMessage(self.mcp.list()));
        }

        if let Some(server_name) = input.strip_prefix("tools ") {
            return Ok(AgentEvent::SystemMessage(
                self.mcp.list_tools(server_name.trim())?,
            ));
        }

        if let Some(input) = input.strip_prefix("call ") {
            let mut parts = input.splitn(3, ' ');
            let server = parts.next().unwrap_or("").trim();
            let tool = parts.next().unwrap_or("").trim();
            let arguments = parts.next().unwrap_or("{}").trim();

            if server.is_empty() || tool.is_empty() {
                return Ok(AgentEvent::SystemMessage(
                    "用法：/mcp call server_name tool_name {\"key\":\"value\"}".to_string(),
                ));
            }

            let arguments = serde_json::from_str(arguments).context("MCP 参数必须是 JSON")?;
            return Ok(AgentEvent::SystemMessage(
                self.mcp.call_tool(server, tool, arguments).await?,
            ));
        }

        Ok(AgentEvent::SystemMessage(
            "用法：/mcp list，/mcp tools 名称，/mcp call 名称 工具 JSON".to_string(),
        ))
    }

    fn handle_rag_command(&mut self, input: &str) -> Result<AgentEvent> {
        if input == "sources" {
            return Ok(AgentEvent::SystemMessage(self.retrieval.list_sources()));
        }

        if input == "reindex" {
            self.retrieval.reindex()?;
            return Ok(AgentEvent::SystemMessage(
                "已重建 RAG 外部数据源索引。".to_string(),
            ));
        }

        if let Some(query) = input.strip_prefix("search ") {
            return Ok(AgentEvent::SystemMessage(
                self.retrieval.format_search_results(query.trim(), 5),
            ));
        }

        if let Some(input) = input.strip_prefix("add-folder ") {
            let mut parts = input.splitn(2, ' ');
            let name = parts.next().unwrap_or("").trim();
            let path = parts.next().unwrap_or("").trim();

            if name.is_empty() || path.is_empty() {
                return Ok(AgentEvent::SystemMessage(
                    "用法：/rag add-folder 名称 路径".to_string(),
                ));
            }

            let source = self.retrieval.add_folder(name, path)?;
            return Ok(AgentEvent::SystemMessage(format!(
                "已添加 RAG 数据源 #{}：{} -> {}",
                source.id,
                source.name,
                source.path.display()
            )));
        }

        if let Some(id) = input.strip_prefix("remove ") {
            let id: usize = id.trim().parse().context("RAG 数据源编号必须是数字")?;
            return Ok(AgentEvent::SystemMessage(self.retrieval.remove(id)?));
        }

        Ok(AgentEvent::SystemMessage(
            "用法：/rag sources，/rag add-folder 名称 路径，/rag remove 编号，/rag reindex，/rag search 关键词".to_string(),
        ))
    }

    fn handle_todo_command(&mut self, input: &str) -> Result<Vec<AgentEvent>> {
        if let Some(title) = input.strip_prefix("add ") {
            let item = self.todo.add(title.trim())?;
            return Ok(vec![
                AgentEvent::SystemMessage(format!("已添加待办 #{}：{}", item.id, item.title)),
                AgentEvent::TodoUpdated,
            ]);
        }

        if input == "list" {
            return Ok(vec![AgentEvent::SystemMessage(self.todo.list())]);
        }

        if let Some(id) = input.strip_prefix("done ") {
            let id: usize = id.trim().parse().context("待办编号必须是数字")?;
            return Ok(vec![
                AgentEvent::SystemMessage(self.todo.done(id)?),
                AgentEvent::TodoUpdated,
            ]);
        }

        Ok(vec![AgentEvent::SystemMessage(
            "用法：/todo add 任务，/todo list，/todo done 编号".to_string(),
        )])
    }

    async fn plan_todos(&mut self, goal: &str) -> Result<Vec<AgentEvent>> {
        let prompt = "你是任务规划助手。请把用户目标拆成 3 到 6 个简短待办。只输出待办列表，每行一个，不要编号，不要解释。";
        let plan = self.llm.ask_with_system(prompt, goal).await?;
        let mut created = Vec::new();

        for line in plan.lines() {
            let title = line
                .trim()
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .trim();

            if !title.is_empty() {
                created.push(self.todo.add(title)?.title);
            }
        }

        if created.is_empty() {
            Ok(vec![AgentEvent::SystemMessage(
                "没有生成待办，请换一个更具体的目标。".to_string(),
            )])
        } else {
            let mut output = String::from("已生成待办：");
            for title in created {
                output.push_str(&format!("\n- {title}"));
            }
            Ok(vec![
                AgentEvent::SystemMessage(output),
                AgentEvent::TodoUpdated,
            ])
        }
    }

    async fn run_sub_agent(&self, input: &str) -> Result<AgentEvent> {
        let mut parts = input.splitn(2, ' ');
        let name = parts.next().unwrap_or("").trim();
        let task = parts.next().unwrap_or("").trim();

        if name.is_empty() || task.is_empty() {
            return Ok(AgentEvent::SystemMessage(
                "用法：/subagent 名称 问题，比如 /subagent reviewer 检查 src/main.rs".to_string(),
            ));
        }

        let answer = self
            .sub_agents
            .run(name, &self.llm, task, &self.tools)
            .await?;
        Ok(AgentEvent::SystemMessage(format!(
            "子代理 {name}：\n{answer}"
        )))
    }

    async fn run_agent_turn(
        &mut self,
        events: &mut Vec<AgentEvent>,
        observer: &mut impl FnMut(&AgentEvent) -> Result<()>,
    ) -> Result<()> {
        let tool_definitions = self.tool_definitions();
        let mut repeated_calls: HashMap<String, usize> = HashMap::new();

        for _ in 0..MAX_AGENT_STEPS {
            let turn = self
                .llm
                .ask_with_tools(&self.messages, tool_definitions.clone())
                .await?;
            let _finish_reason = turn.finish_reason.as_deref();

            if turn.tool_calls.is_empty() {
                let answer = turn.content.unwrap_or_default();
                emit_event(
                    events,
                    observer,
                    AgentEvent::AssistantMessage(answer.clone()),
                )?;
                hooks::after_agent_answer(&mut self.messages, answer);
                return Ok(());
            }

            let assistant_content = turn
                .content
                .clone()
                .filter(|content| !content.trim().is_empty());
            if let Some(content) = assistant_content.as_ref() {
                emit_event(
                    events,
                    observer,
                    AgentEvent::AssistantMessage(content.clone()),
                )?;
            }

            self.messages.push(Message::assistant_tool_calls(
                assistant_content,
                turn.tool_calls.clone(),
            ));

            for tool_call in turn.tool_calls {
                let tool_name = tool_call.function.name.clone();
                let tool_input = match self
                    .tools
                    .iter()
                    .find(|tool| tool.name == tool_name)
                    .map(|tool| tools::tool_arguments_to_input(tool, &tool_call.function.arguments))
                {
                    Some(Ok(input)) => input,
                    Some(Err(error)) => {
                        let error_message = error.to_string();
                        emit_event(
                            events,
                            observer,
                            AgentEvent::ToolError {
                                name: tool_name.clone(),
                                error: error_message.clone(),
                            },
                        )?;
                        self.messages
                            .push(Message::tool_result(tool_call.id, error_message));
                        continue;
                    }
                    None => {
                        let error_message = format!("DeepSeek 想调用未知工具：{tool_name}");
                        emit_event(
                            events,
                            observer,
                            AgentEvent::ToolError {
                                name: tool_name.clone(),
                                error: error_message.clone(),
                            },
                        )?;
                        self.messages
                            .push(Message::tool_result(tool_call.id, error_message));
                        continue;
                    }
                };

                let call_key = format!("{tool_name}\n{tool_input}");
                let call_count = repeated_calls.entry(call_key).or_default();
                *call_count += 1;
                if *call_count > MAX_REPEATED_TOOL_CALLS {
                    let error_message = format!(
                        "已阻止重复工具调用：{tool_name} 使用完全相同的参数连续出现超过 {MAX_REPEATED_TOOL_CALLS} 次。请分析已有结果、换一种方法或向用户说明阻塞原因。"
                    );
                    emit_event(
                        events,
                        observer,
                        AgentEvent::ToolError {
                            name: tool_name.clone(),
                            error: error_message.clone(),
                        },
                    )?;
                    self.messages
                        .push(Message::tool_result(tool_call.id, error_message));
                    continue;
                }

                emit_event(
                    events,
                    observer,
                    AgentEvent::ToolCall {
                        name: tool_name.clone(),
                        input: tool_input.clone(),
                    },
                )?;
                emit_event(
                    events,
                    observer,
                    AgentEvent::StatusChanged(AgentStatus::RunningTool(tool_name.clone())),
                )?;

                match self.execute_agent_tool(&tool_name, &tool_input).await {
                    Ok(tool_output) => {
                        self.messages
                            .push(Message::tool_result(tool_call.id, tool_output.clone()));
                        emit_event(
                            events,
                            observer,
                            AgentEvent::ToolResult {
                                name: tool_name.clone(),
                                output: tool_output,
                            },
                        )?;
                        if is_todo_tool(&tool_name) {
                            emit_event(events, observer, AgentEvent::TodoUpdated)?;
                        }
                    }
                    Err(error) => {
                        let error_message = error.to_string();
                        self.messages
                            .push(Message::tool_result(tool_call.id, error_message.clone()));
                        emit_event(
                            events,
                            observer,
                            AgentEvent::ToolError {
                                name: tool_name.clone(),
                                error: error_message,
                            },
                        )?;
                    }
                }
            }

            emit_event(
                events,
                observer,
                AgentEvent::StatusChanged(AgentStatus::Thinking),
            )?;
        }

        emit_event(
            events,
            observer,
            AgentEvent::SystemMessage(format!(
                "工具执行已达到 {MAX_AGENT_STEPS} 步上限，正在根据已有结果生成最终总结。"
            )),
        )?;

        let mut final_messages = self.messages.clone();
        final_messages.push(Message::new(
            "system",
            "你已经达到本轮工具调用上限。不要再调用工具。请根据已有工具结果给出诚实、完整的最终回答：说明已完成内容、验证结果、未完成事项和阻塞原因。",
        ));
        let answer = self.llm.ask(&final_messages).await?;
        emit_event(
            events,
            observer,
            AgentEvent::AssistantMessage(answer.clone()),
        )?;
        hooks::after_agent_answer(&mut self.messages, answer);
        Ok(())
    }

    fn compact_history_if_needed(&mut self) -> bool {
        let total_chars: usize = self.messages.iter().map(message_size).sum();
        if total_chars <= COMPACT_HISTORY_CHARS || self.messages.len() <= KEEP_RECENT_MESSAGES + 1 {
            return false;
        }

        let mut start = self.messages.len().saturating_sub(KEEP_RECENT_MESSAGES);
        while start > 1 && self.messages[start].role != "user" {
            start -= 1;
        }

        let mut compacted = Vec::with_capacity(self.messages.len() - start + 2);
        compacted.push(self.messages[0].clone());
        compacted.push(Message::new(
            "system",
            "较早的会话内容已因上下文长度限制被压缩。长期偏好由 Memory 提供，当前执行状态由 Todo 提供；请以最近消息和实际工具结果为准。",
        ));
        compacted.extend(self.messages[start..].iter().cloned());
        self.messages = compacted;
        true
    }

    fn refresh_system_prompt(&mut self) {
        if let Some(system_message) = self.messages.first_mut() {
            system_message.content = Some(build_agent_system_prompt(
                &self.tools,
                &self.skills,
                &self.sub_agents,
                &self.workspace,
                &self.mcp,
            ));
        }
    }

    async fn execute_agent_tool(&mut self, tool_name: &str, tool_input: &str) -> Result<String> {
        if tool_name.starts_with("mcp__") {
            let arguments = if tool_input.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(tool_input).context("MCP 工具参数必须是 JSON")?
            };
            return self.mcp.call_qualified_tool(tool_name, arguments).await;
        }

        match tool_name {
            "skill_load" => self.execute_skill_load(tool_input),
            "memory_add" => self.execute_memory_add(tool_input),
            "todo_add" => self.execute_todo_add(tool_input),
            "todo_update" => self.execute_todo_update(tool_input),
            "todo_done" => self.execute_todo_done(tool_input),
            "todo_list" => Ok(self.todo.list()),
            "dispatch_subagent" => self.execute_dispatch_subagent(tool_input).await,
            _ => tools::execute_tool(self.llm.http_client(), tool_name, tool_input).await,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions: Vec<ToolDefinition> = self
            .tools
            .iter()
            .map(|tool| {
                ToolDefinition::function(
                    tool.name,
                    format!("{}。用法参考：{}", tool.description, tool.usage),
                    tool.parameters.clone(),
                )
            })
            .collect();
        definitions.extend(self.mcp.tools().into_iter().map(|tool| {
            ToolDefinition::function(
                tool.qualified_name,
                format!(
                    "MCP Server `{}` 的工具 `{}`：{}",
                    tool.server_name, tool.tool_name, tool.description
                ),
                tool.input_schema,
            )
        }));
        definitions
    }

    fn execute_skill_load(&mut self, input: &str) -> Result<String> {
        let request: SkillLoadInput = serde_json::from_str(input)
            .context("skill_load 输入必须是 JSON，例如 {\"name\":\"code-review\"}")?;
        self.skills.load(&request.name)
    }

    fn execute_memory_add(&mut self, input: &str) -> Result<String> {
        let request = parse_memory_add_input(input)?;
        validate_memory_candidate(&request.content)?;
        let item = self
            .memory
            .add(request.kind.trim(), request.content.trim())?;
        Ok(format!("已保存长期记忆 #{}：{}", item.id, item.content))
    }

    fn execute_todo_add(&mut self, input: &str) -> Result<String> {
        let titles = parse_todo_add_titles(input)?;
        let mut created = Vec::new();

        for title in titles {
            validate_todo_title(&title)?;
            let item = self.todo.add(&title)?;
            created.push(format!("#{} {}", item.id, item.title));
        }

        Ok(format!("已添加待办：\n{}", created.join("\n")))
    }

    fn execute_todo_update(&mut self, input: &str) -> Result<String> {
        self.todo.update_all_from_json(input)
    }

    fn execute_todo_done(&mut self, input: &str) -> Result<String> {
        let request: TodoDoneInput =
            serde_json::from_str(input).context("todo_done 输入必须是 JSON，例如 {\"id\":1}")?;
        self.todo.done(request.id)
    }

    async fn execute_dispatch_subagent(&self, input: &str) -> Result<String> {
        let request: DispatchSubAgentInput = serde_json::from_str(input)
            .context("dispatch_subagent 输入必须是 JSON，例如 {\"agent_type\":\"researcher\",\"task\":\"搜索资料\"}")?;
        let purpose = request
            .purpose
            .as_deref()
            .filter(|purpose| !purpose.trim().is_empty())
            .unwrap_or("未填写 purpose");
        let answer = self
            .sub_agents
            .run(&request.agent_type, &self.llm, &request.task, &self.tools)
            .await?;

        Ok(format!(
            "子代理 `{}` 已完成。\n目的：{}\n任务：{}\n\n{}",
            request.agent_type, purpose, request.task, answer
        ))
    }
}

fn build_agent_system_prompt(
    tools: &[Tool],
    skills: &SkillRegistry,
    sub_agents: &SubAgentRegistry,
    workspace: &WorkspaceContext,
    mcp: &McpRegistry,
) -> String {
    let tool_descriptions: Vec<String> = tools
        .iter()
        .map(|tool| {
            format!(
                "- {} [{}]：{}，用法参考：{}",
                tool.name,
                tool_source_label(&tool.source),
                tool.description,
                tool.usage
            )
        })
        .collect();

    let mcp_descriptions = mcp
        .tools()
        .iter()
        .map(|tool| format!("- {}：{}", tool.qualified_name, tool.description))
        .collect::<Vec<_>>();

    format!(
        "你是一个耐心的 Rust 学习助手，回答要简洁、清楚。\n\
当前日期：{}。\n\
当前时区：Asia/Shanghai。\n\
{}\n\
你可以根据用户问题自主决定是否调用工具，用户不需要输入工具命令。工具调用必须使用 API 提供的原生 function calling，不要把工具调用 JSON 当作普通文本输出。\n\
可用工具：\n{}\n\
可用子代理：\n{}\n\
可用 Skill（需要时调用 skill_load 按需加载完整说明）：\n{}\n\
已连接 MCP 工具：\n{}\n\
如果不需要工具，直接正常回答。如果需要工具，调用对应工具并等待工具结果，再继续判断下一步，直到任务完成。\n\
工具选择规则：\n\
- 如果用户要求查看当前项目文件或目录，使用 ls/read；大文件或需要指定范围时使用 read_lines。\n\
- 如果用户要求理解项目结构、寻找相关文件、判断应该改哪里，优先使用 repo_map 获取仓库地图，再按需 read 具体文件。\n\
- 如果需要定位定义、引用、错误文字或某段代码，使用 search_text 搜索，再用 read/read_lines 查看相关文件，不要靠猜测文件位置。\n\
- 如果用户明确要求创建、写入或修改当前项目文件，使用 mkdir/write_file/append_file/replace_in_file。\n\
- 如果用户要求检查编译、格式化或测试，使用 run_command 或 validate_project；修改 Rust 代码后优先使用 validate_project 做完整校验。\n\
- 如果当前目录是 Git 仓库，可以用 git_status 和 git_diff 检查变更；这两个工具只读。\n\
- 如果用户要求搜索网页，先使用 web_search；如果用户说今天、最新、最近等相对时间，搜索关键词要结合当前日期；如果需要阅读某个搜索结果的正文，再使用 web_fetch。\n\
- 如果用户要求根据外部资料、知识库、笔记、文档库、RAG 数据源回答，使用 rag_search，并把 input 写成适合检索的关键词。\n\
- 如果任务需要独立研究、代码审查、规划拆解或 Rust 教学，可以调用 dispatch_subagent 派遣合适子代理；子代理会用独立上下文完成子任务并返回总结。\n\
- 不要为一句话就能回答的小问题派遣子代理；不要让子代理嵌套派遣子代理。\n\
- 如果用户表达了稳定偏好、长期目标或项目事实，使用 memory_add 保存长期记忆。\n\
- 如果用户提出明确任务、计划、待执行事项或要求你规划步骤，使用 todo_add 创建待办，或用 todo_update 全量维护任务状态。\n\
- 开始执行某个待办时，用 todo_update 将它标记为 in_progress；完成时用 todo_update 或 todo_done 标记 done；阻塞时用 todo_update 标记 blocked。\n\
- 不要要求用户手动输入 /rag_search；这是调试命令，不是自主工具调用方式。\n\
自动 Memory/Todo 安全规则：\n\
- 不要保存 API Key、密码、token、密钥等敏感信息。\n\
- 不要把临时闲聊、一次性问题、模型猜测写入长期记忆。\n\
- Memory 只保存稳定偏好、长期目标、项目事实。\n\
- Todo 只保存具体、可执行的任务。\n\
Todo 执行规则：\n\
- 当前待办是你的任务状态，不是装饰信息。\n\
- 当用户请求和当前待办相关时，优先推进待办。\n\
- 同一时间最多一个待办是 in_progress。\n\
- 如果完成了某个待办，必须调用 todo_update 或 todo_done 更新状态。\n\
- 如果发现目标需要拆分，调用 todo_add 添加具体后续任务。\n\
任务执行规则：\n\
- 如果用户要求创建或修改文件，工具成功后应继续检查结果，例如 read/ls/repo_map 或 validate_project，而不是调用一次工具就结束。\n\
- 如果工具返回错误，分析错误并尝试修正；如果无法修正，明确告诉用户阻塞原因。\n\
- 不要用完全相同的参数反复调用同一个工具；已有结果不足时，应调整参数、换工具或说明阻塞。\n\
- 最终回答要总结实际完成了什么、涉及哪些文件或命令、是否还有未完成事项。",
        current_date_for_prompt(),
        workspace.prompt_section(),
        tool_descriptions.join("\n"),
        sub_agents.prompt_summary(),
        skills.catalog_for_prompt(),
        if mcp_descriptions.is_empty() {
            "（没有已连接的 MCP 工具）".to_string()
        } else {
            mcp_descriptions.join("\n")
        }
    )
}

fn message_size(message: &Message) -> usize {
    let content_chars = message
        .content
        .as_deref()
        .map(str::chars)
        .map(Iterator::count)
        .unwrap_or_default();
    let tool_chars: usize = message
        .tool_calls
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|call| call.function.name.chars().count() + call.function.arguments.chars().count())
        .sum();
    content_chars + tool_chars
}

fn current_date_for_prompt() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let china_days = ((seconds + 8 * 60 * 60) / 86_400) as i64;
    let (year, month, day) = civil_from_days(china_days);

    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };

    (year as i32, m as u32, d as u32)
}

fn parse_memory_add_input(input: &str) -> Result<MemoryAddInput> {
    if input.trim().starts_with('{') {
        return serde_json::from_str(input)
            .context("memory_add 输入必须是 JSON，例如 {\"kind\":\"preference\",\"content\":\"用户偏好中文回答\"}");
    }

    Ok(MemoryAddInput {
        kind: default_memory_kind(),
        content: input.trim().to_string(),
    })
}

fn parse_todo_add_titles(input: &str) -> Result<Vec<String>> {
    if input.trim().starts_with('{') {
        let request: TodoAddInput = serde_json::from_str(input).context(
            "todo_add 输入必须是 JSON，例如 {\"title\":\"实现自动记忆\"} 或 {\"titles\":[\"任务1\",\"任务2\"]}",
        )?;

        let mut titles = request.titles;
        if !request.title.trim().is_empty() {
            titles.push(request.title);
        }

        let titles: Vec<String> = titles
            .into_iter()
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty())
            .collect();

        if titles.is_empty() {
            return Err(anyhow::anyhow!("todo_add 至少需要一个 title"));
        }

        return Ok(titles);
    }

    let title = input.trim().to_string();
    if title.is_empty() {
        Err(anyhow::anyhow!("todo_add 标题不能为空"))
    } else {
        Ok(vec![title])
    }
}

fn validate_memory_candidate(content: &str) -> Result<()> {
    let content = content.trim();

    if content.chars().count() < 4 {
        return Err(anyhow::anyhow!("记忆内容太短，不适合作为长期记忆"));
    }

    if looks_sensitive(content) {
        return Err(anyhow::anyhow!(
            "拒绝保存疑似密钥、密码、token 或其他敏感信息"
        ));
    }

    let stable_signals = [
        "偏好",
        "喜欢",
        "希望",
        "长期",
        "目标",
        "项目",
        "使用",
        "正在学习",
        "以后",
        "默认",
        "记住",
    ];

    if !stable_signals.iter().any(|signal| content.contains(signal)) {
        return Err(anyhow::anyhow!(
            "这条内容不像稳定偏好、长期目标或项目事实，暂不写入长期记忆"
        ));
    }

    Ok(())
}

fn validate_todo_title(title: &str) -> Result<()> {
    let title = title.trim();

    if title.chars().count() < 2 {
        return Err(anyhow::anyhow!("待办标题太短"));
    }

    if looks_sensitive(title) {
        return Err(anyhow::anyhow!("待办标题不能包含疑似密钥或敏感信息"));
    }

    Ok(())
}

fn looks_sensitive(content: &str) -> bool {
    let lower = content.to_lowercase();
    lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("token")
        || lower.contains("sk-")
        || lower.contains("密钥")
        || lower.contains("密码")
}

fn default_memory_kind() -> String {
    "fact".to_string()
}

fn is_todo_tool(tool_name: &str) -> bool {
    matches!(tool_name, "todo_add" | "todo_update" | "todo_done")
}

fn emit_event(
    events: &mut Vec<AgentEvent>,
    observer: &mut impl FnMut(&AgentEvent) -> Result<()>,
    event: AgentEvent,
) -> Result<()> {
    observer(&event)?;
    events.push(event);
    Ok(())
}

fn observed_result(
    source_events: Vec<AgentEvent>,
    observer: &mut impl FnMut(&AgentEvent) -> Result<()>,
) -> Result<AgentTurnResult> {
    let mut result = AgentTurnResult::new();

    for event in source_events {
        emit_event(&mut result.events, observer, event)?;
    }

    Ok(result)
}

fn format_tool_list(tools: &[Tool]) -> String {
    let mut output = String::from("可用工具：");

    for tool in tools {
        output.push_str(&format!(
            "\n- {} [{}]：{}，用法：{}",
            tool.name,
            tool_source_label(&tool.source),
            tool.description,
            tool.usage
        ));
    }

    output
}

fn tool_source_label(source: &ToolSource) -> &'static str {
    match source {
        ToolSource::Local => "local",
    }
}
