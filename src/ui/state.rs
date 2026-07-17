use crate::agent::{AgentEvent, AgentStatus, TodoSnapshotItem};
use crate::mcp::McpServerSnapshot;
use crate::skills::SkillSnapshotItem;

pub struct TuiState {
    pub input: String,
    pub transcript: Vec<TranscriptItem>,
    pub todos: Vec<TodoSnapshotItem>,
    pub skills: Vec<SkillSnapshotItem>,
    pub mcp_servers: Vec<McpServerSnapshot>,
    pub local_tool_count: usize,
    pub status: UiStatus,
    pub command_hint: String,
    pub session_id: String,
    pub scroll_offset: usize,
}

pub enum UiStatus {
    Ready,
    Thinking,
    RunningTool(String),
    Error(String),
}

pub enum TranscriptItem {
    User(String),
    Assistant(String),
    Thinking(String),
    ToolCall { name: String, input: String },
    ToolResult { name: String, output: String },
    System(String),
    Error(String),
}

impl TuiState {
    pub fn new(
        todos: Vec<TodoSnapshotItem>,
        skills: Vec<SkillSnapshotItem>,
        mcp_servers: Vec<McpServerSnapshot>,
        local_tool_count: usize,
    ) -> Self {
        Self {
            input: String::new(),
            transcript: Vec::new(),
            todos,
            skills,
            mcp_servers,
            local_tool_count,
            status: UiStatus::Ready,
            command_hint: command_hint(""),
            session_id: session_id(),
            scroll_offset: 0,
        }
    }

    pub fn apply_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::UserMessage(content) => {
                self.transcript.push(TranscriptItem::User(content.clone()));
            }
            AgentEvent::AssistantMessage(content) => {
                self.transcript
                    .push(TranscriptItem::Assistant(content.clone()));
            }
            AgentEvent::ToolCall { name, input } => {
                self.transcript.push(TranscriptItem::ToolCall {
                    name: name.clone(),
                    input: input.clone(),
                });
            }
            AgentEvent::ToolResult { name, output } => {
                self.transcript.push(TranscriptItem::ToolResult {
                    name: name.clone(),
                    output: output.clone(),
                });
            }
            AgentEvent::ToolError { name, error } => {
                self.transcript
                    .push(TranscriptItem::Error(format!("工具 {name} 出错：{error}")));
            }
            AgentEvent::SystemMessage(content) => {
                self.transcript
                    .push(TranscriptItem::System(content.clone()));
            }
            AgentEvent::TodoUpdated => {
                self.transcript
                    .push(TranscriptItem::System("TodoList 已更新。".to_string()));
            }
            AgentEvent::RuntimeSnapshot {
                todos,
                skills,
                mcp_servers,
            } => {
                self.todos = todos.clone();
                self.skills = skills.clone();
                self.mcp_servers = mcp_servers.clone();
            }
            AgentEvent::StatusChanged(status) => {
                self.status = UiStatus::from(status);
                match status {
                    AgentStatus::Thinking => self
                        .transcript
                        .push(TranscriptItem::Thinking("正在分析请求...".to_string())),
                    AgentStatus::RunningTool(name) => self
                        .transcript
                        .push(TranscriptItem::Thinking(format!("准备执行工具 `{name}`"))),
                    AgentStatus::Ready | AgentStatus::Error(_) => {}
                }
            }
        }

        self.trim_transcript();
    }

    pub fn reset_session(
        &mut self,
        todos: Vec<TodoSnapshotItem>,
        skills: Vec<SkillSnapshotItem>,
        mcp_servers: Vec<McpServerSnapshot>,
    ) {
        self.input.clear();
        self.transcript.clear();
        self.todos = todos;
        self.skills = skills;
        self.mcp_servers = mcp_servers;
        self.status = UiStatus::Ready;
        self.command_hint = command_hint("");
        self.session_id = session_id();
        self.scroll_offset = 0;
    }

    pub fn refresh_command_hint(&mut self) {
        self.command_hint = command_hint(&self.input);
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = usize::MAX;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn has_conversation(&self) -> bool {
        self.transcript.iter().any(|item| {
            matches!(
                item,
                TranscriptItem::User(_)
                    | TranscriptItem::Assistant(_)
                    | TranscriptItem::ToolCall { .. }
                    | TranscriptItem::ToolResult { .. }
                    | TranscriptItem::Error(_)
            )
        })
    }

    pub fn message_count(&self) -> usize {
        self.transcript
            .iter()
            .filter(|item| matches!(item, TranscriptItem::User(_) | TranscriptItem::Assistant(_)))
            .count()
    }

    pub fn tool_call_count(&self) -> usize {
        self.transcript
            .iter()
            .filter(|item| matches!(item, TranscriptItem::ToolCall { .. }))
            .count()
    }

    pub fn estimated_tokens(&self) -> usize {
        let chars: usize = self
            .transcript
            .iter()
            .map(|item| match item {
                TranscriptItem::User(content)
                | TranscriptItem::Assistant(content)
                | TranscriptItem::Thinking(content)
                | TranscriptItem::System(content)
                | TranscriptItem::Error(content) => content.chars().count(),
                TranscriptItem::ToolCall { name, input } => {
                    name.chars().count() + input.chars().count()
                }
                TranscriptItem::ToolResult { name, output } => {
                    name.chars().count() + output.chars().count()
                }
            })
            .sum();

        chars / 4
    }

    pub fn todo_counts(&self) -> (usize, usize) {
        let done = self
            .todos
            .iter()
            .filter(|todo| todo.status == "Done")
            .count();
        let pending = self.todos.len().saturating_sub(done);
        (pending, done)
    }

    fn trim_transcript(&mut self) {
        let max_items = 200;
        if self.transcript.len() > max_items {
            let start = self.transcript.len() - max_items;
            self.transcript.drain(0..start);
        }
    }
}

impl From<&AgentStatus> for UiStatus {
    fn from(status: &AgentStatus) -> Self {
        match status {
            AgentStatus::Ready => UiStatus::Ready,
            AgentStatus::Thinking => UiStatus::Thinking,
            AgentStatus::RunningTool(name) => UiStatus::RunningTool(name.clone()),
            AgentStatus::Error(error) => UiStatus::Error(error.clone()),
        }
    }
}

fn command_hint(input: &str) -> String {
    if !input.starts_with('/') {
        return "Enter 发送 | Esc/Ctrl+C 退出 | 输入 / 查看指令".to_string();
    }

    let commands = [
        "/help",
        "/tools",
        "/todo list",
        "/todo add ",
        "/todo done ",
        "/memory list",
        "/memory add ",
        "/skills",
        "/skill use ",
        "/subagents",
        "/subagent ",
        "/new",
        "/clear",
        "/quit",
    ];

    let matches: Vec<&str> = commands
        .iter()
        .copied()
        .filter(|command| command.starts_with(input) || input == "/")
        .take(6)
        .collect();

    if matches.is_empty() {
        "未匹配到指令，Enter 后交给 Agent 处理".to_string()
    } else {
        format!("指令：{}", matches.join("  "))
    }
}

fn session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();

    format!("{:x}", seconds)
}
