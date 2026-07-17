# Architecture

这个项目采用“先能跑，再逐步拆清楚”的结构。当前重点是让每个模块职责单一，方便后续继续扩展。

## 目录结构

```text
src/
  main.rs
  config.rs
  agent.rs
  hooks/
    mod.rs
  llm.rs
  message.rs

  tools/
    mod.rs
    calc.rs
    command.rs
    file.rs
    git.rs
    repo.rs
    search.rs
    web.rs

  memory/
    mod.rs

  retrieval/
    mod.rs

  mcp/
    mod.rs
    protocol.rs

  todo/
    mod.rs

  skills/
    mod.rs

  sub_agent/
    mod.rs

  ui/
    mod.rs
    cli.rs
    tui.rs
    state.rs
```

## main.rs

入口文件，只做三件事：

- 通过 `config` 读取 `DEEPSEEK_API_KEY`
- 创建 `Agent`
- 根据启动参数选择 CLI 或 TUI

目标：保持 `main.rs` 很短，不把业务逻辑塞回去。

当前启动方式：

```powershell
cargo run        # CLI
cargo run -- tui # TUI
```

## config.rs

配置读取模块。

当前负责：

- 优先读取系统环境变量
- 如果系统环境变量不存在，依次读取当前工作目录 `.env` 和用户目录 `.rust-deepseek-agent/.env`
- 给 `DEEPSEEK_API_KEY`、`BRAVE_SEARCH_API_KEY` 这类密钥提供统一读取入口

当前目录配置可覆盖用户级配置；用户级配置支持全局安装后的 Agent 在任意工作目录启动。`.env` 不会提交到 Git。代码不要打印密钥内容。

## agent.rs

主调度中心。

负责：

- 系统命令分发
- 手动工具命令分发
- 普通对话消息管理
- AI 自主工具调用循环
- 连接 Memory、Todo、Skill、Sub-Agent
- 在关键时机调用 Hooks
- 处理单次用户输入并返回 `AgentTurnResult`

核心结构：

```rust
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
}
```

为了同时支持 CLI 和 TUI，Agent 不直接控制终端，而是处理一次用户输入并返回事件：

```rust
agent.handle_user_input(input).await
```

TUI 需要更及时的执行反馈时，使用观察者接口：

```rust
agent.handle_user_input_stream(input, |event| { ... }).await
```

这个接口会在“用户消息、思考中、工具调用、工具结果、最终回答”等事件产生时立即通知 UI。

返回值包含：

```text
AgentTurnResult
  events: Vec<AgentEvent>
  should_exit: bool
```

CLI 复用 `handle_user_input`，TUI 复用 `handle_user_input_stream`，两者共享同一套 Agent 主流程。

普通对话进入 LLM 前，Agent 会先注入长期记忆。RAG 不会在每轮普通对话里自动检索；只有用户执行 `/rag search ...`，或模型自主选择 `rag_search` 工具时，才会检索外部资料源。

Agent 执行循环还包含三层可靠性保护：完全相同的工具调用最多重复两次；达到单轮工具步数上限后关闭工具并生成最终总结；历史消息过长时保留 system prompt 和最近完整消息，避免上下文持续膨胀。

注意：模型自主选择 `rag_search` 时，用户输入的是自然语言问题，不是 `/rag_search` 命令。`/rag_search` 只是手动工具调试入口。

## llm.rs

LLM 客户端模块。

负责：

- DeepSeek API 地址
- 模型名
- 请求结构
- 响应结构
- 发送聊天请求

后续如果要切换模型服务，优先改这里。

## hooks/

轻量 Hook 系统。

Hook 可以理解为“Agent 运行到某个关键时刻时自动触发的逻辑”。当前没有使用复杂 trait，而是先用清晰的普通函数：

```rust
before_llm_user_message
after_tool_result
after_agent_answer
```

当前 Hook 负责：

- `before_llm_user_message`：用户消息进入 LLM 前，注入相关长期记忆。
- `after_tool_result`：手动工具调试入口执行后，把工具输入和结果写入对话上下文。AI 自主工具调用使用原生 `tool_calls` / `role=tool` 消息回填。
- `after_agent_answer`：LLM 正常回答后，把 assistant 消息写入历史。

后续可以继续加入：

- `before_tool_call`
- `after_error`
- `after_agent_turn`
- `after_todo_update`
- `after_memory_added`

等 Hook 数量变多后，再考虑升级成 trait-based Hook 系统。

## message.rs

消息结构模块。

当前支持普通文本消息和原生工具调用消息：

```rust
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}
```

其中 `tool_calls` 用于保存 assistant 发起的 function calling，`tool_call_id` 用于保存 `role=tool` 的工具结果。

后续可以继续扩展出更明确的消息类型，例如：

- 普通用户消息
- 普通 assistant 消息
- 工具调用消息
- 工具结果消息
- 子代理结果消息

## tools/

工具系统。

`tools/mod.rs` 负责：

- 注册工具
- 列出工具
- 执行工具
- 把工具结果写回上下文

具体工具拆分：

- `calc.rs`：计算工具
- `command.rs`：安全白名单命令执行工具
- `file.rs`：项目内文件查看、读取、写入和精确替换工具
- `git.rs`：只读 Git 状态和差异工具
- `repo.rs`：仓库地图工具，提供文件结构和 Rust 符号概览
- `search.rs`：项目内递归文本搜索工具
- `web.rs`：Web Search 和网页正文抓取工具

当前内置工具分组：

- 文件：`ls`、`read`、`repo_map`、`write_file`、`append_file`、`replace_in_file`、`mkdir`
- 命令：`run_command`、`validate_project`
- 网络：`web_search`、`web_fetch`
- 检索：`rag_search`
- 状态管理：`memory_add`、`todo_add`、`todo_update`、`todo_done`、`todo_list`
- 子代理：`dispatch_subagent`
- MCP：`mcp_call`
- 其他：`echo`、`calc`

安全边界：

- 文件工具只能访问当前项目内的相对路径。
- `repo_map` 只扫描当前项目内目录，并跳过 `target`、`.git`、`.agent_data` 等本地状态或构建目录。
- `search_text` 只搜索当前项目内文本，并跳过构建、依赖、Git 和 Agent 本地状态目录；`read_lines` 一次最多读取 400 行。
- 写文件工具不能写到项目目录外。
- 当前没有删除文件工具。
- `git_status` 和 `git_diff` 只执行只读 Git 命令，不提供暂存、提交或还原能力。
- `run_command` 不是任意 shell，只允许 `cargo check/fmt/test/build/clippy`、`cargo --version`、`rustc --version`。
- `validate_project` 固定执行 `cargo fmt`、`cargo check`、`cargo test`，用于 Rust 代码修改后的综合校验。
- `web_search` 优先使用 `BRAVE_SEARCH_API_KEY`，未配置时自动回退到 DuckDuckGo Lite；`web_fetch` 只支持 http/https URL。
- `memory_add`、`todo_add`、`todo_update`、`todo_done`、`todo_list`、`dispatch_subagent` 是 Agent 内部工具，由 `agent.rs` 执行，因为它们需要访问 Agent 持有的状态或调度器。
- 自动 Memory/Todo 写入会拒绝疑似 API Key、密码、token、密钥等敏感信息。

每个工具现在都有 JSON Schema 参数定义，用于 DeepSeek 原生 function calling。底层执行仍然保留函数分发形式：

```rust
match tool_name {
    "calc" => calc::run(tool_input),
    "ls" => file::list_files(tool_input),
    "run_command" => command::run(tool_input),
    ...
}
```

后续可以升级成 trait-based tool：

```rust
trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn run(&self, input: &str) -> Result<String>;
}
```

## memory/

长期记忆模块。

当前实现：

- 保存到 `.agent_data/memory.json`
- 支持添加、列出、关键词搜索
- 普通对话前尝试注入相关记忆
- 支持 AI 通过 `memory_add` 自主写入稳定长期记忆

当前召回策略很简单：关键词匹配。

自动写入规则：

- 只记录稳定偏好、长期目标、项目事实。
- 不记录临时闲聊、一次性问题或模型猜测。
- 不记录密钥、密码、token 等敏感信息。

后续升级方向：

- 给记忆加标签
- 给记忆加重要性分数
- 定期压缩对话为长期记忆
- 使用 embedding 做相似度召回

## retrieval/

外部资料 RAG 检索模块。

当前实现：

- 从 `.agent_data/rag_sources.json` 读取用户添加的数据源
- 支持添加本地文件夹作为外部资料源
- 扫描已注册数据源里的文本文件
- 按固定行数切片
- 使用关键词匹配打分
- 支持 `/rag sources`
- 支持 `/rag add-folder 名称 路径`
- 支持 `/rag remove 编号`
- 支持 `/rag reindex`
- 支持 `/rag search 关键词`
- 提供工具 `rag_search` 给模型自主调用

当前没有使用 embedding 或向量数据库，方便学习和本地运行。RAG 不负责自动扫描当前项目源码，也不在普通对话前自动注入内容。

## mcp/

MCP 外部工具桥接模块。

当前实现：

- 从 `.agent_data/mcp_servers.json` 读取 MCP Server 配置
- 提供 `/mcp list`
- 提供 `/mcp tools server_name`
- 提供 `/mcp call server_name tool_name JSON`
- 提供通用工具 `mcp_call`

当前 MCP 调用使用 stdio JSON-RPC 的第一版实现。后续可以升级成长连接、缓存工具列表和更完整的协议能力。

## todo/

任务规划模块。

当前实现：

- Todo 只存在于当前 Agent 会话内，不写入磁盘
- 支持添加、列出、完成
- 支持 `/plan` 让 LLM 拆解目标并写入 TodoList
- 支持 AI 通过 `todo_add`、`todo_update`、`todo_done`、`todo_list` 自主管理任务
- pending/in_progress Todo 会注入普通对话上下文，用来指导 AI 优先推进当前任务，并在开始、完成或阻塞时更新状态
- 程序重启或执行 `/new`、`/clear` 后清空 Todo；长期 Memory 独立保留

当前状态：

```rust
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
    Blocked,
}
```

后续可以加入：

```rust
Cancelled
```

并让 Agent 在执行任务时自动更新状态。

## skills/

技能系统。

当前 Skill 是内置 prompt：

- `rust_teacher`
- `code_reviewer`
- `planner`

启用 Skill 后，会刷新主 Agent 的 system prompt。

后续可以升级成文件加载：

```text
skills/
  rust_teacher/
    skill.toml
    prompt.md
```

这样就能把技能变成可扩展能力包。

## sub_agent/

子代理系统。

当前子代理是带独立上下文、工具白名单和最大执行轮数的小型 Agent：

- `rust_teacher`
- `reviewer`
- `planner`
- `researcher`

调用方式：

```text
/subagent reviewer 检查 src/main.rs
```

主 Agent 也可以通过原生 function calling 调用 `dispatch_subagent`，自动派遣合适子代理。子代理执行完成后只返回总结，不把自己的完整中间上下文塞进主 Agent。

当前边界：

- 子代理可以调用白名单内的普通工具，例如 `ls`、`read`、`web_search`、`web_fetch`、`rag_search`、`run_command`。
- 子代理不能直接修改主 Agent 的 Memory/Todo。
- 子代理不嵌套派遣子代理。
- 主 Agent 负责最终决策、文件修改和 Todo/Memory 状态更新。

后续升级方向：

- 子代理之间协作
- 多个子代理结果汇总

## ui/

TUI 设计见 [`TUI_DESIGN.md`](TUI_DESIGN.md)。

`ui/` 模块只负责：

- CLI/TUI 输入输出
- TUI 绘制
- 键盘事件
- UI 状态
- 把用户输入交给 Agent
- 展示 Agent 返回的事件

`ui/` 不应该直接实现：

- LLM 调用
- Tool 逻辑
- Memory 逻辑
- Todo 业务逻辑
- Skill/Sub-Agent 业务逻辑

推荐依赖方向：

```text
main -> ui
ui -> agent
agent -> llm/tools/memory/retrieval/mcp/todo/skills/sub_agent/hooks
tools -> retrieval/mcp
sub_agent -> llm/message/tools
```

当前文件：

- `ui/cli.rs`：普通命令行循环，把 `AgentEvent` 打印成文本。
- `ui/tui.rs`：TUI 主循环、键盘事件和 ratatui 绘制；当前采用 OpenCode 风格的欢迎页、主工作区、右侧状态栏和底部输入框。
- `ui/state.rs`：TUI 展示状态、聊天记录、Todo 快照、命令提示和右侧栏统计信息。
- `ui/mod.rs`：UI 模块入口。

## Agent 自主工具调用流程

```text
用户输入普通问题
-> Agent::handle_user_input
-> Memory 注入相关长期记忆
-> Todo 注入当前 pending/in_progress 任务和执行规则
-> 写入 messages
-> LLM 根据 tools JSON Schema 判断是否需要 function calling
-> 如果直接回答：显示回答并保存到 messages
-> 如果返回 tool_calls：Rust 执行对应工具
-> 工具结果以 role=tool 写入 messages
-> 再问 LLM
-> LLM 可继续调用工具，或给出最终回答
-> Agent 返回 AgentEvent
```

这套循环目前最多允许连续执行 16 步，避免模型陷入无限工具调用。

工具不只包括文件、Web、RAG，也包括内部状态工具。例如：

```json
{"tool":"memory_add","input":"{\"kind\":\"preference\",\"content\":\"用户偏好默认使用中文回答\"}"}
```

```json
todo_update 可用于全量维护任务状态，例如把某个任务标记为 in_progress 或 done。
```

如果用户说“根据我的外部资料解释 async trait”，LLM 应该自主返回：

```json
{"tool":"rag_search","input":"async trait"}
```

然后由 Rust 执行检索并把结果交回 LLM。

## 数据文件

运行后会生成：

```text
.agent_data/
  memory.json
  rag_sources.json
```

这些是本地状态文件，不建议提交到 Git。

## 推荐下一步

1. 把工具系统进一步升级成 trait-based tool。
2. 给 RAG 增加 embedding、外部数据库或更多数据源类型。
3. 给 MCP 增加长连接和工具列表缓存。
4. 给 TUI 增加更完整的 slash command 补全。
5. 给 Memory 增加自动总结功能。
6. 把 Skill 从内置代码迁移为本地文件加载。
7. 增加子代理执行流展示和更细的子任务状态。
