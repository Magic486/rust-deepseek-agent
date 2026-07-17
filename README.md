# rust-deepseek-agent

一个用 Rust 编写的 DeepSeek Agent 学习项目，支持普通 CLI 和实验性 TUI。

开发或让 AI assistant 修改本项目之前，请先阅读：

- [AGENT_GUIDE.md](AGENT_GUIDE.md)：根目录入口，指向详细协作文档
- [docs/AGENT_GUIDE.md](docs/AGENT_GUIDE.md)：协作规则、模块边界和扩展流程
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)：项目结构和架构说明
- [docs/DOC_SYNC.md](docs/DOC_SYNC.md)：代码变更后的文档同步规则
- [docs/TUI_DESIGN.md](docs/TUI_DESIGN.md)：TUI 界面设计和实现阶段规划
- [docs/RAG_MCP.md](docs/RAG_MCP.md)：RAG 检索和 MCP 接入说明

这个项目的目标不是一次性做出复杂框架，而是循序渐进地理解 Agent 的核心组成：

- LLM 对话
- 工具调用
- AI 自主决定是否调用工具
- 轻量 Hook
- 长期记忆
- TodoList 任务规划
- Skill 技能模式
- Sub-Agent 子代理
- RAG 外部资料检索
- MCP 外部工具桥接

## 运行前准备

在 PowerShell 中进入项目目录：

```powershell
cd C:\Users\yelfs\Desktop\计算机探索\rust
```

在 `.env` 中填写 DeepSeek API Key：

```env
DEEPSEEK_API_KEY=你的 DeepSeek API Key
```

Web Search 不强制要求搜索 API Key。没有 `BRAVE_SEARCH_API_KEY` 时，项目会自动使用 DuckDuckGo Lite 免费兜底；如果你想要更稳定的搜索结果，可以在 `.env` 中填写 Brave Search API Key：

```env
BRAVE_SEARCH_API_KEY=你的 Brave Search API Key
```

配置读取顺序是：系统环境变量、当前工作目录的 `.env`、用户目录下的 `.rust-deepseek-agent/.env`。如果要把编译后的 Agent 当作全局命令在任意文件夹启动，推荐把密钥放在：

```text
C:\Users\你的用户名\.rust-deepseek-agent\.env
```

当前目录的 `.env` 适合项目专用配置，用户目录配置适合所有工作目录共用。两种文件都不要提交到 Git。

检查项目：

```powershell
cargo check
```

运行项目：

```powershell
cargo run
```

启动 TUI：

```powershell
cargo run -- tui
```

TUI 快捷键：

```text
Enter       发送输入
PgUp/PgDown 滚动聊天记录
Up/Down     小步滚动聊天记录
Home/End    跳到顶部/底部
Esc/Ctrl+C 退出并恢复终端
/           显示 slash command 提示
```

TUI 采用接近 OpenCode 的暗色 coding agent 布局：空会话欢迎页、左侧聊天/执行流、右侧 Session/Context/Todo 状态栏、底部输入框。Agent 执行过程中会即时显示“思考中”、工具调用和工具结果；最终回答仍然在模型返回后展示。

## 常用命令

退出：

```text
exit
```

查看系统命令：

```text
/help
```

查看工具：

```text
/tools
```

查看 RAG 外部数据源：

```text
/rag sources
```

添加一个本地文件夹作为 RAG 数据源：

```text
/rag add-folder Rust文档 C:\Users\yelfs\Documents\rust-notes
```

重建 RAG 索引：

```text
/rag reindex
```

检索外部数据源：

```text
/rag search async trait
```

## 工具系统

可以手动调用工具。这主要用于调试，确认某个工具本身能不能跑：

```text
/calc 12 * 8
/ls
/ls src
/read src/main.rs
/mkdir notes
/write_file {"path":"notes/a.md","content":"hello","overwrite":false}
/append_file {"path":"notes/a.md","content":"\nmore"}
/replace_in_file {"path":"notes/a.md","old":"hello","new":"hi","replace_all":false}
/run_command cargo check
/web_search Rust async tutorial
/web_fetch https://www.rust-lang.org
/rag_search async trait
/memory_add {"kind":"preference","content":"用户偏好默认使用中文回答"}
/todo_add {"titles":["实现自动记忆","实现自动 Todo"]}
/todo_update {"todos":[{"id":1,"title":"实现自动记忆","status":"in_progress"}]}
/todo_done {"id":1}
/todo_list
/mcp_call {"server":"demo","tool":"hello","arguments":{}}
```

日常使用时，更推荐用自然语言让 AI 自主决定是否调用工具：

```text
帮我看看 src 目录下面有什么文件
帮我算一下 12 乘以 8
读取一下 src/main.rs 并总结它做了什么
帮我创建 notes/todo.md 并写入三个学习任务
帮我运行 cargo check 检查项目
搜索 Rust async tutorial，并读取最相关的网页
根据我的外部资料，解释一下 async trait 为什么复杂
以后默认用中文回答
帮我规划实现自动记忆和自动 Todo
```

这时你不需要输入 `/rag_search`、`/memory_add` 或 `/todo_add`。如果 AI 判断需要外部资料、长期记忆或待办管理，它会通过 DeepSeek 原生 function calling 调用工具；Rust 负责执行工具，再把 `tool` 结果交回 AI 继续判断下一步，直到生成最终回答。

当前工具包括：

- `echo`：原样返回输入内容
- `calc`：计算两个数字
- `ls`：查看当前项目内的文件夹
- `read`：读取当前项目内的文本文件
- `read_lines`：按行号分段读取大文件，最多一次读取 400 行
- `repo_map`：生成当前项目的仓库地图，列出文件结构和 Rust 符号概览
- `search_text`：递归搜索项目文本，返回文件、行号和匹配内容
- `write_file`：在当前项目内写入文件，默认不覆盖已有文件
- `append_file`：给当前项目内已有文本文件追加内容
- `replace_in_file`：精确替换当前项目内文本文件中的内容
- `mkdir`：在当前项目内创建目录
- `run_command`：运行安全白名单命令，例如 `cargo check/fmt/test/build`
- `validate_project`：依次执行 `cargo fmt`、`cargo check`、`cargo test`
- `git_status`：只读查看 Git 分支和工作区状态
- `git_diff`：只读查看未暂存或已暂存的 Git 差异
- `web_search`：搜索网页；优先使用 Brave Search API，未配置密钥时自动改用 DuckDuckGo Lite
- `web_fetch`：读取 http/https 网页正文
- `rag_search`：检索用户添加的外部 RAG 数据源
- `memory_add`：AI 自主保存稳定长期记忆
- `todo_add`：AI 自主添加一个或多个待办
- `todo_update`：AI 自主全量更新 Todo 状态，支持 `pending`、`in_progress`、`done`、`blocked`
- `todo_done`：AI 自主标记待办完成
- `todo_list`：AI 自主查看待办列表
- `dispatch_subagent`：AI 自主派遣独立子代理处理研究、审查、规划或 Rust 讲解任务
- `mcp_call`：通过 `.agent_data/mcp_servers.json` 中配置的 MCP Server 调外部工具

文件工具只允许访问当前项目内的相对路径，避免误读或误写系统目录。当前没有删除文件工具。`repo_map` 和 `search_text` 会跳过 `target`、`.git`、`.agent_data` 等目录。`git_status`、`git_diff` 只读取仓库状态，不会提交、暂存或还原文件。`run_command` 不是任意 shell，只允许安全白名单命令；`validate_project` 是改完 Rust 代码后的综合校验入口。

Agent 会阻止同一工具使用完全相同参数反复执行。达到单轮工具步数上限时，它会停止调用工具，并基于已有结果生成最终总结。对话上下文过长时会保留系统规则和最近的完整消息，避免请求无限膨胀；Todo 仍只属于当前会话，长期偏好由 Memory 保存。

Memory/Todo 工具是内部状态管理工具。手动命令保留用于调试和纠正，日常使用时应该让 AI 根据自然语言自主判断是否写入记忆、创建待办或更新待办状态。自动写入会拒绝疑似 API Key、密码、token、密钥等敏感信息。

## RAG 外部资料检索

RAG 用来检索用户添加的外部资料源，例如笔记、课程资料、产品文档、外部代码参考库。

数据源配置保存到：

```text
.agent_data/rag_sources.json
```

当前不会在每次普通对话前自动检索 RAG。你可以通过 `/rag search ...` 手动检索；也可以直接用自然语言提问，例如“根据我的外部资料解释 async trait”，让 AI 自主决定是否调用 `rag_search`。

## MCP 外部工具桥接

MCP Server 配置文件：

```text
.agent_data/mcp_servers.json
```

示例：

```json
[
  {
    "name": "demo",
    "command": "some-mcp-server",
    "args": []
  }
]
```

命令：

```text
/mcp list
/mcp tools demo
/mcp call demo tool_name {"key":"value"}
```

## Memory 长期记忆

Memory 会保存到：

```text
.agent_data/memory.json
```

命令：

```text
/memory add 我正在学习 Rust agent 项目
/memory list
/memory search Rust
```

Agent 在普通对话时，会尝试把相关长期记忆注入当前问题里。AI 也可以在用户表达稳定偏好、长期目标或项目事实时，主动调用 `memory_add` 写入长期记忆。

## TodoList 任务规划

TodoList 是当前会话的任务状态，不写入磁盘。程序重启或开始新会话后，Todo 会从空列表开始；长期 Memory 不受影响。

命令：

```text
/todo add 拆分 agent 项目结构
/todo list
/todo done 1
```

让 AI 生成任务计划：

```text
/plan 学会 Rust agent 开发
```

AI 也可以在普通对话中主动调用 `todo_add`、`todo_update`、`todo_done`、`todo_list` 管理任务。Todo 支持 `pending`、`in_progress`、`done`、`blocked` 四种状态；当前 pending/in_progress Todo 会注入普通对话上下文，AI 会优先推进相关待办，并在开始、完成或阻塞时更新状态。`/todo ...` 命令主要用于手动查看、纠正和调试。

使用 `/new` 或 `/clear` 开始新会话，会一起清空对话上下文、TUI 记录和当前 Todo。

## Skill 技能系统

查看技能：

```text
/skills
```

启用技能：

```text
/skill use rust_teacher
/skill use code_reviewer
/skill use planner
```

Skill 的本质是给主 Agent 增加一段额外 system prompt，让它以某种模式工作。

## Sub-Agent 子代理

查看子代理：

```text
/subagents
```

调用子代理：

```text
/subagent rust_teacher 解释一下 Rust 的 Result
/subagent reviewer 检查 src/main.rs 的结构
/subagent planner 帮我规划下一步学习路线
```

当前子代理包括：

- `rust_teacher`：讲解 Rust 概念和代码，可读取项目文件、检索 RAG 或网页资料
- `reviewer`：审查代码风险和可维护性，可读取文件和运行安全检查命令
- `planner`：拆解目标和规划步骤，可读取项目文件和外部资料
- `researcher`：研究网页、外部资料和背景信息，可使用 web/RAG 工具

日常使用时不需要手动输入 `/subagent`。主 Agent 可以通过 `dispatch_subagent` 工具自主派遣合适的子代理。子代理有独立上下文、工具白名单和最大执行轮数；它不会直接修改主 Agent 的 Memory/Todo，而是把结论返回给主 Agent 汇总。

## 学习重点

这个项目目前最重要的结构是：

```text
用户输入
-> CLI/TUI 收集输入
-> Agent 判断是不是系统命令
-> Agent 判断是不是手动工具命令
-> 普通输入交给 LLM
-> LLM 通过原生 function calling 决定是否调用工具
-> Rust 执行工具
-> 工具结果以 role=tool 进入上下文
-> LLM 继续判断下一步，直到生成最终回答
-> Agent 返回事件
-> CLI 打印事件，TUI 渲染事件
```

这就是一个最小 Agent 闭环。

## Hook 系统

Hook 是 Agent 运行到某个关键时刻时自动触发的逻辑。

当前项目有三个轻量 Hook：

- `before_llm_user_message`：用户消息进入 LLM 前注入相关记忆
- `after_tool_result`：手动工具调试入口执行后把结果写进上下文；AI 自主工具调用使用原生 `role=tool` 消息
- `after_agent_answer`：AI 正常回答后保存 assistant 消息

当前 Hook 还只是普通函数，不是复杂插件系统。这样更适合学习阶段，也方便以后逐步升级。
