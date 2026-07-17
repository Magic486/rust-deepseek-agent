# TUI Design

这份文档描述 rust-deepseek-agent 的 TUI 设计。

目标是做一个类似现代 coding agent 终端界面的体验：既能聊天，也能看到任务、工具调用、执行过程和 `/` 指令。

本文件描述 TUI 设计，并记录当前实现状态。

## 目标

TUI 第一阶段目标：

- 保留现有 Agent 能力。
- 支持聊天输入和回答展示。
- 支持 `/` 指令。
- 展示 TodoList。
- 实时显示 Agent 的执行状态。
- 实时显示工具调用、工具结果和错误。
- 为后续展示 Memory、Skill、Sub-Agent、AgentTeam 留出空间。

非目标：

- 第一版不做复杂鼠标交互。
- 第一版不做多 tab 编辑器。
- 第一版不做文件 diff 编辑器。
- 第一版不做完整 IDE。
- 第一版不做复杂动画。

## 推荐技术栈

推荐使用：

```toml
ratatui = "0.30"
crossterm = "0.29"
```

职责：

- `ratatui`：布局、组件、文本渲染。
- `crossterm`：终端 raw mode、键盘事件、进入/退出 alternate screen。

## 总体原则

TUI 只做外壳。

```text
TUI = 显示 + 输入 + 事件
Agent = LLM + Tool + Memory + Todo + Skill + Sub-Agent + Hook
```

TUI 不应该直接修改：

- Memory
- Todo
- Tool
- Skill
- Sub-Agent

TUI 应该通过 Agent 暴露的接口来触发行为。

## 预期启动方式

保留普通 CLI：

```powershell
cargo run
```

新增 TUI：

```powershell
cargo run -- tui
```

后续也可以支持：

```powershell
cargo run -- --ui tui
cargo run -- --ui cli
```

第一版可以先用最简单的参数判断，不急着引入 `clap`。

## 推荐目录结构

```text
src/
  ui/
    mod.rs
    cli.rs
    tui.rs
    state.rs
```

职责：

```text
ui/mod.rs
  UI 模块入口。

ui/cli.rs
  当前普通命令行模式。后续把 Agent::run() 的 CLI 循环迁移到这里。

ui/tui.rs
  TUI 主循环。负责 terminal 初始化、绘制、键盘事件循环。

ui/state.rs
  TUI 状态。保存输入框、聊天记录、Todo 展示、当前状态、事件流和 transcript 滚动偏移。

ui/event.rs
  暂未创建。等键盘事件和应用事件变复杂时再拆。
```

## 关键重构要求

在实现 TUI 前，应该先把 Agent 从“直接控制终端输入输出”改成“处理一次用户输入并返回事件”。

旧模式类似：

```rust
agent.run().await
```

目标模式：

```rust
agent.handle_user_input(input).await
```

推荐返回：

```rust
AgentTurnResult
```

示意：

```rust
pub struct AgentTurnResult {
    pub events: Vec<AgentEvent>,
    pub should_exit: bool,
}
```

事件示意：

```rust
pub enum AgentEvent {
    UserMessage(String),
    AssistantMessage(String),
    ToolCall { name: String, input: String },
    ToolResult { name: String, output: String },
    ToolError { name: String, error: String },
    TodoUpdated,
    StatusChanged(AgentStatus),
}
```

这样 CLI 和 TUI 都能复用同一个 Agent：

```text
CLI 负责把 AgentEvent 打印成文本。
TUI 负责把 AgentEvent 渲染到界面。
```

## 当前界面布局

当前 TUI 采用接近 OpenCode 的暗色 coding agent 布局。

```text
┌──────────────────────────────────────────────┬──────────────────────┐
│ Chat / Execution                             │ Session / Context     │
│                                              │ Tools                 │
│ ▌ 你                                         │ Todo                  │
│   ...                                        │ Shortcuts             │
│                                              │                      │
│ ◇ 工具                                       │                      │
│ ┌─ tool                                      │                      │
│ │ 调用 read                                  │                      │
│ └────────────────────                        │                      │
├──────────────────────────────────────────────┤                      │
│ ▌ input                                      │                      │
│ rust-deepseek-agent · DeepSeek · Ready       │                      │
└──────────────────────────────────────────────┴──────────────────────┘
```

区域：

```text
Welcome
  空会话时显示弹性居中的大标题、输入提示、快捷键提示和 tip，风格参考 OpenCode 欢迎页。此时不渲染底部固定输入框，避免出现两个输入框或视觉干扰。

Main Panel
  展示聊天历史、Agent 回答、工具调用、工具结果和错误。

Right Panel
  展示 Session、Context、Tools、Todo 和 Shortcuts。

Input Panel
  产生会话事件后显示底部固定输入框，展示当前输入、模型名、状态和命令提示。
```

窄终端下会隐藏右侧栏，只保留主区域和输入区。

## 后续界面布局

后续可以继续增强右侧信息栏：

```text
Session
Context
Tools
Todo
Memory
Active Skill
Sub-Agent
```

右侧栏可展示：

- 当前启用 Skill
- 可用工具
- 最近 Memory
- 当前 Sub-Agent
- 当前执行状态

## 状态设计

推荐 TUI 状态：

```rust
pub struct TuiState {
    pub input: String,
    pub transcript: Vec<TranscriptItem>,
    pub todos: Vec<TodoViewItem>,
    pub status: UiStatus,
    pub command_suggestions: Vec<String>,
    pub selected_panel: Panel,
}
```

示意类型：

```rust
pub enum UiStatus {
    Ready,
    Thinking,
    RunningTool(String),
    Error(String),
}

pub enum TranscriptItem {
    User(String),
    Assistant(String),
    ToolCall { name: String, input: String },
    ToolResult { name: String, output: String },
    System(String),
    Error(String),
}
```

Todo 展示不要直接暴露内部 `TodoItem`，可以做一个轻量视图类型：

```rust
pub struct TodoViewItem {
    pub id: usize,
    pub title: String,
    pub status: String,
}
```

## 实时显示 Agent 思考和执行

注意：这里的“思考”不是展示模型隐藏推理链。

TUI 应展示的是**可观察执行过程**：

- 当前正在分析用户请求
- AI 决定调用哪个工具
- 工具输入是什么
- 工具执行结果是什么
- 是否发生错误
- 最终回答是什么

推荐显示：

```text
Thinking...
Tool call: ls
Input: src
Tool result:
[文件] main.rs
Agent:
src 目录下有一个 main.rs 文件...
```

不推荐显示：

```text
模型内部完整思考链
```

原因：

- 很多模型不会提供真实内部推理。
- 显示隐藏推理链会让界面和用户预期变复杂。
- 可观察执行流已经足够让用户理解 Agent 在做什么。

## Slash Command 设计

输入 `/` 后，TUI 应进入 slash command 模式。

第一版命令：

```text
/help
/tools
/todo list
/todo add ...
/todo done ...
/memory list
/memory add ...
/skills
/skill use ...
/subagents
/subagent ...
/new
/clear
/quit
```

`/new` 和 `/clear` 都表示开始新会话：清空 Agent 对话上下文、TUI transcript 和会话 Todo，并生成新的 session id。长期 Memory 保留。

建议交互：

```text
用户输入 /
-> 状态栏展示可用命令
-> 用户继续输入 /todo
-> 候选收窄到 /todo list, /todo add, /todo done
-> Enter 执行
```

第一版可以先不做复杂补全，只做：

- 输入 `/` 时显示命令提示
- Enter 后复用现有系统命令处理逻辑

第二版再做：

- 上下键选择命令
- Tab 补全
- 命令参数提示

## TodoList 展示

TodoList 是 TUI 的核心面板之一。

展示格式：

```text
Todo
[ ] #1 拆分 agent 项目结构
[x] #2 添加 Hook
[ ] #3 设计 TUI
```

状态颜色建议：

```text
Pending     普通色
InProgress 亮色或强调色
Done        暗色
Blocked    红色或警告色
```

当前代码已有：

```rust
Pending
InProgress
Done
Blocked
```

如果 TUI 要继续扩展任务执行状态，后续可以增加：

```rust
Cancelled
```

## Agent 执行流

Agent 应通过事件驱动 UI 更新。

理想流程：

```text
用户按 Enter
-> TUI 添加 User transcript
-> TUI 状态变为 Thinking
-> Agent 判断是否调用工具
-> TUI 收到 ToolCall 事件
-> TUI 状态变为 RunningTool
-> 工具执行完成
-> TUI 收到 ToolResult 事件
-> Agent 继续调用 LLM
-> TUI 收到 AssistantMessage 事件
-> TUI 状态变为 Ready
```

第一版可以先同步等待 Agent 返回，再一次性把事件渲染出来。

第二版再做真正实时流：

```text
Agent 在执行过程中不断发送 AgentEvent
TUI 收到事件后立即刷新
```

当前状态：TUI 已通过 `handle_user_input_stream` 实现事件级实时刷新。它会在用户消息、思考状态、工具调用、工具结果和最终回答产生时重绘界面。当前还不是 token 级流式输出，模型最终回答仍然等一次 LLM 请求完成后展示。

## Transcript 渲染

当前 TUI 已把一条 `TranscriptItem` 渲染成多行消息块，而不是单行日志。

当前展示规则：

- 用户、Agent、系统、错误、工具调用、工具结果分别使用不同标题和颜色。
- 用户和 Agent 消息使用左侧彩色竖线强调。
- 消息块之间保留空行，避免内容挤在一起。
- 工具结果保留原始换行，不再压缩成一行。
- 长工具结果会限制展示行数，并显示省略提示；当前阈值较高，避免过早截断 Agent 输出和工具结果。
- 工具调用和工具结果使用接近 coding agent 的小卡片样式展示。
- Agent 回答支持轻量 Markdown 渲染，包括标题、列表、引用、分割线和 fenced code block。
- 空会话时显示欢迎页；产生会话事件后切换到工作区视图。
- Transcript 支持键盘滚动：`PgUp/PgDown` 大步滚动，`Up/Down` 小步滚动，`Home/End` 跳到顶部/底部。滚动范围按自动换行后的实际屏幕行数计算，长文本也能完整滚到底部。

这只是 UI 展示层变化，不改变 `AgentEvent`、工具执行逻辑或 Agent 主流程。

## 推荐实现阶段

### Phase 1: TUI 设计和依赖

- 新增 `docs/TUI_DESIGN.md`
- 更新 README 和架构文档链接
- 添加 `ratatui`、`crossterm` 依赖

状态：已完成。

### Phase 2: 拆分 CLI

- Agent 新增 `handle_user_input`。已开始。
- Agent 新增 `AgentEvent` / `AgentTurnResult`。已开始。
- CLI 先通过 `Agent::run()` 调用 `handle_user_input` 并打印事件。已开始。
- 新增 `src/ui/cli.rs`
- 把当前 `Agent::run()` 的终端循环迁移到 CLI UI
- 保持 `cargo run` 行为不变

状态：已完成。CLI 现在位于 `src/ui/cli.rs`，并复用 `Agent::handle_user_input`。

### Phase 3: 最小 TUI

- 新增 `src/ui/tui.rs`
- 支持聊天记录
- 支持输入框
- 支持 Enter 发送
- 支持 Esc/Ctrl+C 退出
- 支持同步等待 Agent 回答

状态：已完成第一版。当前 TUI 已支持事件级实时刷新，但还不是 token 级流式输出。

### Phase 4: Todo 面板

- TUI 左侧展示 TodoList
- `/todo` 命令执行后刷新 Todo 面板
- `/plan` 执行后刷新 Todo 面板

状态：已完成第一版。Todo 通过 `Agent::todo_snapshot()` 暴露给 UI；pending/in_progress Todo 也会注入 Agent 普通对话上下文，用来指导 AI 优先推进相关任务。

### Phase 5: 执行流面板

- 展示 ToolCall
- 展示 ToolResult
- 展示 ToolError
- 展示 Agent status

状态：已完成第一版。当前显示可观察执行流，不显示模型隐藏推理链；工具调用和工具结果会按事件即时刷新。

### Phase 6: Slash Command 体验

- 输入 `/` 显示命令提示
- 支持简单候选列表
- 支持 Tab 补全

状态：部分完成。已支持输入 `/` 后显示候选提示，Tab 补全尚未实现。

### Phase 7: 更像 coding agent 的体验

- 展示活动工具
- 展示当前 Skill
- 展示最近 Memory
- 支持子代理执行流
- 支持任务状态实时更新

状态：已完成 OpenCode 风格骨架。当前包括欢迎页、主工作区、右侧状态栏、底部输入框、消息竖线和工具卡片；尚未实现 Tab agent 切换、Ctrl+P 命令面板和 token 级流式输出。

## Agent 接口建议

为了支持 TUI，Agent 应逐步从“打印输出”改成“返回事件”。

当前：

```rust
println!("工具 {}：\n{}\n", tool_call.tool, tool_output);
```

当前过渡目标：

```rust
events.push(AgentEvent::ToolResult {
    name: tool_call.tool,
    output: tool_output,
});
```

当前状态：

```text
Agent 已新增 handle_user_input。
Agent 已新增 handle_user_input_stream。
Agent 已新增 AgentEvent / AgentTurnResult。
CLI 已迁移到 src/ui/cli.rs。
TUI 已迁移到 src/ui/tui.rs。
```

CLI：

```text
把事件打印成文本
```

TUI：

```text
把事件渲染成界面块
```

这个重构是实现 TUI 的关键。

## 依赖边界

推荐依赖：

```text
main -> ui
ui -> agent
agent -> llm/tools/memory/todo/skills/sub_agent/hooks
```

不推荐：

```text
agent -> ui
tools -> ui
memory -> ui
todo -> ui
llm -> ui
```

原因：

```text
UI 可以依赖业务能力。
业务能力不应该知道自己被 CLI 还是 TUI 使用。
```

## 文档同步要求

实现 TUI 时需要同步：

```text
README.md
  增加 TUI 启动方式和快捷键。

docs/ARCHITECTURE.md
  增加 ui/ 模块职责和依赖方向。

docs/AGENT_GUIDE.md
  增加 UI 模块扩展规则。

docs/TUI_DESIGN.md
  随实现阶段更新设计状态。
```

按照 `docs/DOC_SYNC.md`，只做和 TUI 直接相关的最小文档更新。

## 第一版验收标准

第一版 TUI 完成时，应满足：

- `cargo run` 仍能启动 CLI。
- `cargo run -- tui` 能启动 TUI。
- TUI 能显示聊天记录。
- TUI 能输入消息并发送给 Agent。
- TUI 能展示 Agent 回答。
- TUI 能展示 TodoList。
- TUI 能显示工具调用和工具结果。
- TUI 支持 `/` 指令输入。
- Esc 或 Ctrl+C 能安全退出并恢复终端。
- `cargo fmt` 和 `cargo check` 通过。
