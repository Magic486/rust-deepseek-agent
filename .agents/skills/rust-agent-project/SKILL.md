---
name: rust-agent-project
description: 用 Rust 构建、调试和迭代 DeepSeek Coding Agent 的可复用工程工作流
---

# Rust Agent Project Skill

## 适用场景

当任务涉及用 Rust 构建或维护一个连接 DeepSeek API 的 Coding Agent 时，加载本 Skill。它适合课程项目、个人实验和本地工作区 Agent，不要求一开始就引入复杂框架。

## 核心架构

保持清晰的单向依赖：

```text
main -> ui -> agent -> llm / message / tools / memory / todo / skills / sub_agent / retrieval / mcp
```

- `main.rs` 只负责启动和选择 CLI/TUI。
- `llm.rs` 只负责 HTTP 请求、响应和模型消息协议。
- `agent.rs` 是调度中心，负责消息历史、工具循环、事件和可靠性保护。
- `tools/` 负责工具注册和执行；文件、命令、Git、Web 等工具不要塞回 `main.rs`。
- `memory/` 保存跨会话稳定事实；`todo/` 保存当前会话的可执行任务。
- `skills/` 从文件发现提示词模式；`sub_agent/` 执行有限职责的独立代理。
- `ui/` 只收集输入并渲染 `AgentEvent`，CLI 和 TUI 共享 Agent 逻辑。
- `retrieval/` 只检索用户配置的外部资料源，不默认扫描 Agent 自身源码。
- `mcp/` 负责外部 MCP Server 连接和动态工具注册。

## Agent 闭环

Agent 不是“请求一次模型然后打印答案”。每轮必须遵循：

1. 接收用户消息，注入工作区指令、相关 Memory 和活动 Todo。
2. 把普通工具和 MCP 工具的 JSON Schema 交给模型，使用 `tool_choice=auto`。
3. 如果模型返回 `assistant.tool_calls`，校验工具名和 JSON 参数。
4. 执行工具，把结果用对应 `tool_call_id` 作为 `role=tool` 消息写回历史。
5. 再次请求模型，直到模型给出最终回答、任务阻塞或达到最大步数。
6. 工具失败时把错误反馈给模型，让模型修正参数或诚实说明阻塞原因。

必须设置最大 Agent 步数、相同工具和相同参数的重复调用上限，并在达到上限时关闭工具能力生成最终总结。不能把工具调用 JSON 当作普通文本解析，也不能在工具成功后直接结束任务。

## 文件化 Skill

每个 Skill 放在：

```text
.agents/skills/<lowercase-name>/SKILL.md
```

正文前使用 YAML frontmatter：

```yaml
---
name: example-skill
description: 一句话说明用途
---
```

Skill 应该写可执行规则、适用条件、输入输出约束和验收方式，不要复制整个架构文档。启动时只发现名称和描述；用户或模型需要时调用 `skill_load` 加载完整正文。用户询问 Skill 数量或列表时，必须调用 `skill_list`，不能依据 system prompt 猜测。

新增 Skill 后，至少验证：

```powershell
cargo run -- /skills
cargo run -- /skill_load example-skill
```

## MCP 接入

优先使用官方 `rmcp` SDK。配置中为每个 Server 保存命令、参数和环境变量；启动时建立持久 stdio 连接，完成初始化，调用 `tools/list`，将远程工具映射为：

```text
mcp__<server>__<tool>
```

模型看到的 MCP 工具必须和本地工具使用同一种 Function Calling 结构。调用时解析 JSON 参数、设置超时、保留 Server 错误，并把返回内容写回 `role=tool`。MCP 不应把某个具体 Server 的业务逻辑写进 Agent；无配置时 UI 应显示未配置，而不是伪造已连接状态。

## TUI 设计

Agent 通过 `AgentEvent` 发出用户消息、思考状态、工具调用、工具结果、错误、回答和运行时快照。TUI 用 Tokio 后台任务执行 Agent，通过 `mpsc` 接收事件，避免网络请求阻塞键盘输入和绘制。

界面至少应包含：

- 左侧可滚动的对话和工具执行流；
- 右侧真实的工具、Skill、MCP、Todo 和上下文状态；
- 底部稳定的输入框，光标必须位于实际输入末端；
- `/` 命令提示、Enter 发送、Esc/Ctrl+C 退出、PgUp/PgDown 滚动。

不要在 TUI 中复制 Agent 主循环，也不要把内部隐藏思维链当作可视化内容。只展示可审计的状态、工具名称、参数摘要、结果和最终回答。

## Windows 启动与密钥

MSVC 目标需要 Visual Studio Build Tools 的 C++ 工作负载和 Windows SDK；VS Code 本身不提供 `link.exe`。如果出现 `link.exe not found`，先安装 Build Tools，再重新打开终端。

配置读取顺序应稳定：系统环境变量优先，其次当前目录 `.env`，最后用户目录的全局配置。`.env` 只保存本机密钥，必须加入 `.gitignore`，不得打印或提交：

```text
DEEPSEEK_API_KEY=...
BRAVE_SEARCH_API_KEY=...
```

启动前先确认当前工作区和配置来源，不要在代码中硬编码密钥。

## 工具安全边界

- 相对文件路径必须解析后仍在当前工作区内；拒绝绝对路径、`..` 越界和危险符号链接。
- 写文件默认不覆盖，覆盖需要显式参数。
- 命令工具使用白名单，例如 `cargo fmt`、`cargo check`、`cargo test`、`cargo clippy`。
- Git 工具默认只读，不提供自动还原、暂存或提交。
- Web Search 只读取环境变量中的搜索密钥；没有 Brave Key 时才使用明确的回退方案。
- Memory 只保存稳定偏好、长期目标和项目事实；Todo 只保存具体可执行任务。
- 不把 API Key、密码、token 或其他敏感信息写入 Memory、Todo、Skill 或日志。

## 完成一个改动的标准流程

1. 先阅读 `AGENT_GUIDE.md`、`docs/AGENT_GUIDE.md`、`docs/ARCHITECTURE.md` 和 `docs/DOC_SYNC.md`。
2. 用 `rg` 定位入口、调用链和相关测试，先确认真实结构。
3. 只在所属模块实现改动，保持公共接口和事件语义清楚。
4. 如果改变工具、依赖、数据路径、命令、Agent 流程或安全边界，按 `DOC_SYNC.md` 同步最小范围文档。
5. 依次运行：

```powershell
cargo fmt --all -- --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
git diff --check
```

6. 对 TUI 改动手动验证窗口缩放、滚动、输入光标、空状态、工具执行和新会话隔离。
7. 对 Skill 改动验证发现、列表工具和按需加载；对 MCP 改动验证无配置、连接失败、工具发现和工具调用。
8. 最终回答必须说明改了什么、验证了什么、仍有哪些限制。

## 常见故障排查

- **模型只回答不执行**：检查工具 Schema、`tool_choice=auto` 和 system prompt；不要只依赖模型输出文本。
- **工具执行一次后停止**：确认 tool call 和 tool result 都写入消息历史，并且执行后再次请求模型。
- **工具 JSON 被打印出来**：说明调用链没有使用原生 `tool_calls`，或 UI 把协议消息当普通文本渲染。
- **Skill 数量不对**：先检查目录和 frontmatter，再让模型调用 `skill_list`，不要从提示词目录猜。
- **TUI 卡住**：把 Agent 放入后台 Tokio task，通过 channel 发事件；绘制线程不要等待 HTTP 请求。
- **Todo 只展示不推进**：让模型拥有 `todo_add`、`todo_update`、`todo_done`，并把活动 Todo 注入每轮上下文。
- **新会话残留 Todo**：Todo 应属于 Agent 实例或会话状态；`/new` 必须清空消息、事件记录和 Todo，但保留长期 Memory。
- **Windows 编译失败**：区分 `link.exe not found` 的环境问题和 Rust 代码错误；安装 MSVC 工具链后重开终端。

## 当前能力的诚实边界

不要把“已注册”写成“已验证可用”。分别说明：本地工具注册、Skill 已发现、MCP 已配置、搜索密钥是否存在、实际 API/Server 是否成功调用。没有实现 token 级 SSE 流式、向量数据库 RAG、权限确认或并行 Agent Team 时，应在报告和最终说明中明确列为后续工作。
