# RAG and MCP

这份文档说明当前项目里的外部 RAG 检索和 MCP 桥接。

## RAG

RAG 用来让 Agent 查用户添加的外部资料源。

适合放入 RAG 的内容：

```text
个人笔记
课程资料
产品文档
外部代码参考库
业务知识库导出的文本
```

数据源配置文件：

```text
.agent_data/rag_sources.json
```

当前实现是本地文件夹关键词检索：

```text
读取已注册数据源 -> 扫描文件 -> 按行切片 -> 关键词打分 -> 返回相关片段
```

命令：

```text
/rag sources
/rag add-folder 名称 路径
/rag remove 编号
/rag reindex
/rag search 关键词
```

也可以通过统一工具层手动调用。这主要用于调试工具：

```text
/rag_search 关键词
```

普通对话不会自动检索 RAG。这样可以避免每轮对话都把无关资料塞进上下文，也能避免 Agent 把当前项目源码和外部资料混在一起。

如果希望 Agent 使用 RAG，可以：

- 手动输入 `/rag search ...`
- 用自然语言提问，让 AI 在判断需要外部资料时自主调用 `rag_search`

自然语言示例：

```text
根据我的外部资料，解释 async trait 为什么复杂
结合我的知识库，总结一下这个库的使用方式
```

这类输入不需要写 `/rag_search`。AI 如果决定使用 RAG，会通过 DeepSeek 原生 function calling 调用 `rag_search`，参数里的 `input` 是适合检索的关键词。

当前 RAG 不是向量数据库版本，暂时没有 embedding。后续可以升级：

- 增加 embedding 模型
- 接入 SQLite/向量数据库
- 支持 PDF、网页、数据库等更多数据源
- 给数据源增加启用/禁用、标签和更新时间

## MCP

MCP 用来接外部工具。

配置文件：

```text
.agent_data/mcp_servers.json
```

示例：

```json
[
  {
    "name": "demo",
    "command": "some-mcp-server",
    "args": [],
    "enabled": true,
    "environment": {}
  }
]
```

命令：

```text
/mcp list
/mcp tools demo
/mcp call demo tool_name {"key":"value"}
```

启动时会使用官方 `rmcp` 客户端连接启用的 Server，并缓存 `tools/list`。发现的工具会转换为：

```text
mcp__demo__tool_name
```

然后和本地工具一起传给 DeepSeek 的 function calling。模型可以自主调用 MCP 工具；`/mcp ...`
命令只用于查看连接、查看工具和手动调试。当前实现已经具备 stdio 持久连接、连接超时、调用超时、
错误状态和工具动态注册；后续可继续加入 Streamable HTTP 和 list-changed 通知刷新。
