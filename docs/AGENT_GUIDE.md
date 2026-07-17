# Agent Guide

这份文档给人类开发者和 AI assistant 使用。

如果要修改这个项目，请先阅读本文件，再阅读 `ARCHITECTURE.md`。本文件关注“怎么改才不把项目结构改乱”。

如果代码改动可能影响文档，请遵守 [`DOC_SYNC.md`](DOC_SYNC.md) 中的同步规则。

如果要实现或修改 TUI，请先阅读 [`TUI_DESIGN.md`](TUI_DESIGN.md)。

## 项目目标

这是一个 Rust + DeepSeek 的 CLI Agent 学习项目。

当前目标是循序渐进地实现一个最小但清晰的 Agent 框架：

- 支持普通对话
- 支持手动工具调用
- 支持 AI 自主工具调用
- 支持轻量 Hook
- 支持长期记忆
- 支持 TodoList 任务规划
- 支持 Skill 模式
- 支持 Sub-Agent
- 支持 RAG 外部资料检索
- 支持 MCP 外部工具桥接

项目优先级：

1. 可读性优先。
2. 每次改动保持可编译。
3. 新功能放到正确模块，不把逻辑塞回 `main.rs`。
4. 先做简单版本，再逐步抽象。

## 当前模块职责

### `src/main.rs`

只负责程序启动。

允许做：

- 声明模块
- 通过 `config` 读取启动必需配置
- 创建 `Agent`
- 根据启动参数调用 `ui::cli::run(agent)` 或 `ui::tui::run(agent)`

不要做：

- 工具逻辑
- LLM 请求逻辑
- Memory/Todo/Skill/Sub-Agent 逻辑
- 用户输入循环
- UI 渲染逻辑

### `src/config.rs`

只负责配置读取。

当前规则：

- 优先读取系统环境变量。
- 如果系统环境变量不存在，再依次读取当前工作目录 `.env` 和用户目录 `.rust-deepseek-agent/.env`。
- 当前目录 `.env` 适合项目专用配置，用户目录 `.rust-deepseek-agent/.env` 适合全局命令共用配置。
- 不要打印密钥内容。
- 不要把 `.env` 提交到 Git。

### `src/agent.rs`

主调度中心。

负责：

- 系统命令分发
- 普通对话流程
- AI 自主工具调用循环
- `handle_user_input` 单次输入处理接口
- `handle_user_input_stream` 事件观察接口，供 TUI 实时刷新
- `AgentEvent` / `AgentTurnResult` 事件结果
- 连接 `llm`、`tools`、`memory`、`retrieval`、`mcp`、`todo`、`skills`、`sub_agent`、`hooks`

注意：

- `agent.rs` 可以协调模块，但不要吞掉模块职责。
- CLI 应复用 `handle_user_input`，TUI 可复用 `handle_user_input_stream`，不要复制一套 Agent 主流程。
- 如果某段逻辑明显属于工具、记忆、任务、技能或子代理，应下沉到对应模块。

### `src/ui/`

UI 外壳。

当前文件：

- `ui/cli.rs`：普通 CLI 输入循环和事件打印。
- `ui/tui.rs`：TUI 初始化、绘制和键盘事件循环。
- `ui/state.rs`：TUI 展示状态、聊天记录、Todo 快照、slash command 提示和 transcript 滚动偏移。
- `ui/mod.rs`：UI 模块入口。

允许做：

- 收集用户输入
- 调用 `agent.handle_user_input(...)` 或 `agent.handle_user_input_stream(...)`
- 把 `AgentEvent` 打印或渲染出来
- 展示 Todo 快照、状态和执行流

不要做：

- 直接调用 DeepSeek API
- 直接执行 Tool
- 直接修改 Memory/Todo/Skill/Sub-Agent 的业务数据
- 复制 `agent.rs` 里的主流程

### `src/llm.rs`

只负责 LLM API 通信。

负责：

- DeepSeek API 地址
- 模型名称
- 请求结构
- 响应结构
- `LlmClient`

不要依赖：

- `agent`
- `tools`
- `memory`
- `todo`
- `skills`
- `sub_agent`

后续切换模型服务时，优先改这里。

### `src/message.rs`

只负责消息结构。

当前核心类型支持普通消息和原生工具调用消息：

```rust
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}
```

`tool_calls` 表示 assistant 请求 function calling，`tool_call_id` 表示工具结果对应的调用编号。后续如果需要子代理消息、摘要消息，优先从这里扩展。

### `src/tools/`

工具系统。

负责：

- 工具注册
- 工具列表
- 手动工具命令
- 工具执行
- 具体工具实现

文件划分：

- `tools/mod.rs`：工具注册和统一入口
- `tools/calc.rs`：计算工具
- `tools/command.rs`：安全白名单命令执行
- `tools/file.rs`：项目内文件查看、按行读取、写入、追加、精确替换和创建目录
- `tools/git.rs`：只读 Git 状态和差异查看
- `tools/repo.rs`：仓库地图工具，提供文件结构和 Rust 符号概览
- `tools/search.rs`：项目内递归文本搜索
- `tools/web.rs`：Web Search 和网页正文抓取
- `memory_add`、`todo_add`、`todo_update`、`todo_done`、`todo_list`、`dispatch_subagent` 虽然注册在工具列表里，但由 `agent.rs` 执行，因为它们需要访问 Agent 持有的状态或调度器。

安全规则：

- 文件工具只能访问当前项目内的相对路径。
- `repo_map` 只生成当前项目的结构概览，不负责外部资料检索；外部资料仍然走 RAG。
- `search_text` 只在项目内搜索并跳过构建、依赖和本地状态目录；大文件用 `read_lines` 分段读取。
- 不要允许默认访问绝对路径。
- 写文件工具不能写到项目目录外。
- `write_file` 默认不覆盖已有文件；需要覆盖时必须显式传入 `overwrite: true`。
- `replace_in_file` 应优先使用精确片段；多处匹配时不要默认全部替换。
- 当前不要加入删除文件工具，除非先设计确认机制。
- `run_command` 必须保持白名单，不要升级成任意 shell。
- `validate_project` 固定执行 `cargo fmt`、`cargo check`、`cargo test`，适合 Rust 代码修改后的综合校验。
- `git_status`、`git_diff` 必须保持只读；新增暂存、提交、还原能力前要先设计权限确认。
- 网络工具不要打印 API Key，搜索密钥只从环境变量读取。
- 自动 Memory/Todo 工具不要记录 API Key、密码、token、密钥等敏感信息。
- 手动 `/memory`、`/todo` 命令是调试和纠正入口，不是通用 Agent 的主要交互方式。

### `src/hooks/`

轻量 Hook 系统。

Hook 是某个关键时机自动触发的逻辑。

当前 Hook：

- `before_llm_user_message`
- `after_tool_result`
- `after_agent_answer`

适合放入 Hook 的逻辑：

- 调用 LLM 前注入记忆
- 工具执行后写入上下文
- 回答结束后保存历史
- 后续自动总结
- 后续错误记录
- 后续自动更新 Todo

不适合放入 Hook 的逻辑：

- 复杂业务主流程
- 具体工具实现
- LLM API 请求实现

### `src/memory/`

长期记忆系统。

当前实现：

- 本地 JSON 存储
- 添加记忆
- 列出记忆
- 关键词搜索
- 对话前注入相关记忆

数据文件：

```text
.agent_data/memory.json
```

注意：

- 记忆属于本地私人数据，不提交到 Git。
- 召回策略目前故意简单，后续再升级。

### `src/todo/`

任务规划系统。

当前实现：

- 会话内存储，不写入本地文件
- 添加待办
- 全量更新待办状态
- 列出待办
- 标记完成
- `/plan` 由 LLM 生成待办
- 支持 `Pending`、`InProgress`、`Done`、`Blocked`

生命周期：程序重启或开始新会话时清空 Todo；长期 Memory 继续跨会话保存。

后续方向：

- 增加删除、取消或重排任务
- 让 Agent 自动更新任务状态

### `src/skills/`

Skill 系统。

当前 Skill 从 `.agents/skills/*/SKILL.md` 发现，并通过 `skill_load` 按需加载。

适合做：

- 改变主 Agent 的回答风格
- 提供领域模式
- 增加 system prompt 约束

当前 Skill：

- `rust-teacher`
- `code-review`
- `task-planner`
- `rust-agent-project`

完整 Skill 正文只在 `skill_load` 后进入当前会话；查询 Skill 数量或描述时通过 `skill_list` 读取真实清单，避免模型根据启动提示词猜测。

### `src/sub_agent/`

子代理系统。

当前 Sub-Agent 是不同角色的独立小型 Agent。每个子代理有：

- 独立 system prompt
- 别名
- 工具白名单
- 最大执行轮数

当前子代理：

- `rust_teacher`
- `reviewer`
- `planner`
- `researcher`

当前调用方式：

```text
/subagent reviewer 检查 src/main.rs
```

主 Agent 也可以通过 `dispatch_subagent` 工具自主派遣子代理。子代理执行完成后只返回总结，不把完整中间上下文塞进主 Agent。

当存在多个互不依赖的只读任务时，优先使用 `dispatch_subagent` 的 `tasks` 数组批量派遣。`SubAgentRegistry::run_many` 会让 `researcher`、`planner`、`rust_teacher` 最多 3 个并发运行；包含 `run_command` 或 `validate_project` 的任务保持串行。每个任务的开始、完成、失败都会转成 `AgentEvent`，结果最后按任务顺序聚合。单任务字段和 `/subagent ...` 仍然可用。

边界：

- 子代理可以调用白名单内的普通工具。
- 子代理不能直接修改主 Agent 的 Memory/Todo。
- 子代理不嵌套派遣子代理。
- 主 Agent 负责最终决策、文件修改和状态更新。
- 并行只适用于相互独立的只读任务；不能让多个子代理同时修改同一工作区或共享状态。

后续方向：

- 在现有批量并发基础上增加依赖 DAG、取消和超时
- 为写操作增加隔离工作区或审批流程

## 依赖方向规则

保持依赖方向清晰，避免循环依赖。

推荐依赖方向：

```text
main -> ui
ui -> agent

agent -> llm
agent -> message
agent -> tools
agent -> memory
agent -> retrieval
agent -> mcp
agent -> todo
agent -> skills
agent -> sub_agent
agent -> hooks

tools -> message
tools -> hooks
tools -> retrieval
tools -> mcp
tools -> config

hooks -> message
hooks -> memory

sub_agent -> llm
sub_agent -> message
sub_agent -> tools

llm -> message
```

不要做：

```text
agent -> ui
llm -> agent
memory -> agent
todo -> agent
tools -> agent
skills -> agent
sub_agent -> agent
```

如果你发现必须从底层模块调用 `Agent`，通常说明职责放错了。

## 新增 Tool 的流程

例如新增 `/time` 工具。

1. 在 `src/tools/` 下创建文件：

```text
src/tools/time.rs
```

2. 在 `src/tools/mod.rs` 里声明模块：

```rust
mod time;
```

3. 在 `registered_tools()` 添加工具信息：

```rust
Tool {
    name: "time",
    description: "查看当前时间",
    usage: "/time",
}
```

4. 在 `execute_tool()` 添加分支：

```rust
"time" => time::run(tool_input),
```

5. 运行：

```powershell
cargo fmt
cargo check
```

注意：

- 如果工具需要网络，用 `reqwest::Client`。
- 如果工具访问文件，必须限制在项目目录内。
- 如果工具可能造成破坏，先设计确认步骤。
- 如果新增命令执行能力，必须默认使用白名单。
- 如果新增写文件能力，必须明确是否允许覆盖，并同步更新文档安全边界。

## 新增 Hook 的流程

1. 在 `src/hooks/mod.rs` 添加函数。

命名建议：

```text
before_xxx
after_xxx
on_xxx
```

2. 在 `agent.rs` 或对应模块的关键时机调用。

3. 保持 Hook 简单。

Hook 应该像：

```rust
hooks::after_tool_result(...)
```

不要让 Hook 变成新的大调度中心。

## 新增 Memory 功能的流程

1. 优先修改 `src/memory/mod.rs`。
2. 如果需要在对话流程中自动触发，再通过 `hooks/` 接入。
3. 如果需要命令入口，再在 `agent.rs` 的系统命令分发中添加。
4. 如果希望 AI 自主写入或查询，优先通过内部工具接入，例如 `memory_add`。

示例：

```text
/memory delete 1
/memory clear
/memory summarize
```

自动 Memory 规则：

- 只记录稳定偏好、长期目标、项目事实。
- 不记录临时闲聊、一次性问题或模型猜测。
- 不记录 API Key、密码、token、密钥等敏感信息。

## 新增 Retrieval 功能的流程

Retrieval 属于外部资料检索，不要和长期记忆、当前项目文件工具混在一起。
如果要理解当前项目结构，使用 `repo_map`；不要把当前项目源码默认塞进 RAG。

优先修改：

```text
src/retrieval/mod.rs
```

适合放入 Retrieval 的逻辑：

- 外部数据源配置
- 外部文档扫描
- 外部文档切片
- 索引构建
- 关键词检索
- 后续 embedding 检索

命令入口放在 `agent.rs`，例如：

```text
/rag sources
/rag add-folder 名称 路径
/rag remove 编号
/rag reindex
/rag search 关键词
```

工具入口放在 `tools/mod.rs`，当前统一工具名是：

```text
rag_search
```

注意：

- 不要让 RAG 默认扫描当前项目的 `src/` 或 `docs/`。
- 不要在每次普通对话前自动检索 RAG。
- 如果要读取当前项目文件，使用 `ls` / `read` 文件工具。
- 如果要查用户提供的外部知识库，使用 `/rag ...` 或 `rag_search`。
- `/rag search ...` 是系统命令，`/rag_search ...` 是手动工具调试命令。
- AI 自主调用 RAG 时，用户不需要输入 `/rag_search`；模型应该在需要时返回 `{"tool":"rag_search","input":"关键词"}`。
- 修改 `rag_search` 行为时，要同步检查 `agent.rs` 里的 system prompt 是否仍然能正确引导模型自主调用。

## 新增 MCP 功能的流程

MCP 属于外部工具桥接，不要把具体 MCP Server 的业务逻辑写进 Agent。

优先修改：

```text
src/mcp/mod.rs
官方 `rmcp` SDK
```

适合放入 MCP 的逻辑：

- MCP Server 配置读取
- `rmcp` 持久 stdio 连接和初始化
- tools/list
- tools/call
- 超时、连接错误和运行时状态快照

Agent 负责把 `McpRegistry` 发现的 `mcp__server__tool` 作为普通 function calling 工具交给模型；
`/mcp ...` 只用于调试，不应要求用户手动描述 MCP 工具参数。

## 新增 Todo 功能的流程

1. 优先修改 `src/todo/mod.rs`。
2. 如果是命令入口，在 `agent.rs` 的 `handle_todo_command` 添加。
3. 如果是自动状态更新，优先考虑 Hook。
4. 如果希望 AI 自主管理任务，优先通过内部工具接入，例如 `todo_add`、`todo_update`、`todo_done`、`todo_list`。

示例：

```text
/todo start 1
/todo block 1 原因
/todo remove 1
```

自动 Todo 规则：

- 只创建具体、可执行的任务。
- 不把普通闲聊或一次性问题写成待办。
- 不把疑似密钥、密码、token 写入待办标题。
- pending/in_progress Todo 会注入普通对话上下文，用来指导 AI 按当前任务推进。
- 如果 AI 开始、完成或阻塞了某个待办，应调用 `todo_update` 或 `todo_done` 更新状态，不要只口头说完成。
- 同一时间最多一个 Todo 是 `InProgress`。
- Todo 属于会话任务状态，不要恢复其他会话的 Todo。

## 新增 Skill 的流程

Skill 采用文件发现和按需加载，不要把业务 Skill 写死在 Rust 源码中。

新增 Skill：

1. 创建 `.agents/skills/<lowercase-name>/SKILL.md`。
2. 添加 `name`、`description` YAML frontmatter，名称必须和目录名一致。
3. 在正文中写可执行规则、适用场景和验收要求，不要复制整个项目文档。
4. 启动 Agent 检查 `/skills`，再用自然语言验证模型是否能自主调用 `skill_load`。

完整正文只在模型调用 `skill_load` 后进入会话，避免启动时膨胀上下文。

## 新增 Sub-Agent 的流程

1. 修改 `src/sub_agent/mod.rs`
2. 在 `SubAgentRegistry::new()` 添加：

```rust
SubAgent {
    name: "new_agent",
    aliases: &["new"],
    description: "...",
    system_prompt: "...",
    tool_names: &["ls", "read"],
    max_turns: 8,
}
```

主 Agent 通过 `dispatch_subagent` 负责调度，`sub_agent` 负责执行子代理。不要让 `sub_agent` 反向依赖 `agent`。

## 什么时候需要 AgentTeam

当前项目已经具备轻量批量调度，不必为了并行简单任务再引入一层包装。只有在需要依赖关系、取消、重试、超时、权限或隔离工作区时，才值得把调度器继续抽象成 AgentTeam。

当前顺序：

1. 手动调用 Sub-Agent。
2. 主 Agent 通过 `dispatch_subagent` 自动派遣一个 Sub-Agent。
3. 主 Agent 批量派遣独立只读 Sub-Agent，并发上限为 3。
4. 为有依赖和写操作的复杂协作抽象 AgentTeam。

AgentTeam 应负责：

- 多代理任务分配
- 多代理结果收集
- 汇总最终回答
- 控制协作轮次

## AI 协作规则

如果 AI assistant 修改本项目，请遵守：

1. 修改前先读 `AGENT_GUIDE.md`。
2. 涉及结构时再读 `ARCHITECTURE.md`。
3. 改代码后按 `DOC_SYNC.md` 判断是否需要同步文档。
4. 不要把业务逻辑塞回 `main.rs`。
5. 新功能放进对应模块。
6. 每次修改后运行：

```powershell
cargo fmt
cargo check
```

7. 不要提交或打印 API Key。
8. 不要让文件工具访问项目外路径。
9. 不要随意删除 `.agent_data/`，它是用户本地状态。
10. 遇到需要网络服务的功能，优先通过 `config::env_var` 读取密钥。
11. 不要把 `run_command` 改成任意 shell，除非先设计权限确认机制。
12. 不要让 AI 自动 Memory/Todo 写入敏感信息。
13. 保持教学友好，宁愿代码直白，也不要过早抽象。
14. 保持 Agent 执行循环的保护：重复调用熔断、步数上限后的最终总结、长上下文压缩。

## 代码风格

当前阶段偏向新手友好。

优先：

- 清楚的函数名
- 小模块
- 简单数据结构
- 明确错误信息
- 每一步能编译

暂缓：

- 复杂 trait
- 宏
- 多层泛型
- 过度异步抽象
- 插件式框架

## 当前推荐演进路线

1. 把工具系统进一步升级成 trait-based Tool。
2. 给 RAG 增加 embedding 或更好的检索评分。
3. 给 MCP 增加 Streamable HTTP、list-changed 通知刷新和更细的权限确认。
4. 给 TUI 增加更完整的 slash command 补全。
5. 给 Memory 增加自动总结功能。
6. 给文件化 Skill 增加热重载、版本校验和 Skill 之间的组合能力。
7. 增加子代理执行流展示和更细的子任务状态。
8. 再实现 AgentTeam。
