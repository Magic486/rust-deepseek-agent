mod calc;
mod command;
mod file;
mod git;
mod repo;
mod search;
mod web;

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde_json::{Value, json};

use crate::retrieval::RetrievalIndex;

#[derive(Clone)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
    pub parameters: Value,
    pub input_format: ToolInputFormat,
    pub source: ToolSource,
}

#[derive(Clone, Copy)]
pub enum ToolSource {
    Local,
}

#[derive(Clone, Copy)]
pub enum ToolInputFormat {
    PlainInput,
    JsonObject,
}

pub fn registered_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "echo",
            description: "原样返回输入内容",
            usage: "/echo hello rust",
            parameters: input_schema("要原样返回的文本"),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "calc",
            description: "计算两个数字，支持 +、-、*、/",
            usage: "/calc 12 * 8",
            parameters: input_schema("数学表达式，例如 12 * 8"),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "ls",
            description: "查看当前项目内的文件夹",
            usage: "/ls 或 /ls src",
            parameters: json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "相对目录路径；留空表示当前项目根目录"
                    }
                }
            }),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "read",
            description: "读取当前项目内的文本文件",
            usage: "/read src/main.rs",
            parameters: input_schema("要读取的项目内相对文件路径"),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "read_lines",
            description: "按行号读取当前项目内文本文件的指定片段；适合继续读取大文件或查看搜索结果附近代码",
            usage: "/read_lines {\"path\":\"src/agent.rs\",\"start_line\":200,\"line_count\":200}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "项目内相对文件路径"},
                    "start_line": {"type": "integer", "minimum": 1, "description": "起始行号，从 1 开始；默认 1"},
                    "line_count": {"type": "integer", "minimum": 1, "maximum": 400, "description": "读取行数；默认 200，最多 400"}
                },
                "required": ["path"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "repo_map",
            description: "生成当前项目的仓库地图，包括文件结构和 Rust 符号概览；适合在改代码前快速理解项目",
            usage: "/repo_map 或 /repo_map src",
            parameters: json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "要扫描的项目内相对目录；留空表示项目根目录"
                    }
                }
            }),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "search_text",
            description: "在当前项目内递归搜索文本并返回文件、行号和匹配行；定位代码时应优先使用",
            usage: "/search_text {\"query\":\"handle_user_input\",\"path\":\"src\",\"case_sensitive\":false}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "要搜索的文本"},
                    "path": {"type": "string", "description": "项目内相对路径；默认当前项目"},
                    "case_sensitive": {"type": "boolean", "description": "是否区分大小写；默认 false"}
                },
                "required": ["query"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "write_file",
            description: "在当前项目内写入文件；默认不覆盖已有文件，输入必须是 JSON",
            usage: "/write_file {\"path\":\"notes/a.md\",\"content\":\"hello\",\"overwrite\":false}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "项目内相对文件路径"},
                    "content": {"type": "string", "description": "要写入的完整文件内容"},
                    "overwrite": {"type": "boolean", "description": "是否覆盖已有文件；默认 false"}
                },
                "required": ["path", "content"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "append_file",
            description: "给当前项目内已有文本文件追加内容，输入必须是 JSON",
            usage: "/append_file {\"path\":\"notes/a.md\",\"content\":\"\\nmore\"}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "项目内相对文件路径"},
                    "content": {"type": "string", "description": "要追加的文本内容"}
                },
                "required": ["path", "content"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "replace_in_file",
            description: "精确替换当前项目内文本文件的一段内容，输入必须是 JSON",
            usage: "/replace_in_file {\"path\":\"src/main.rs\",\"old\":\"旧内容\",\"new\":\"新内容\",\"replace_all\":false}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "项目内相对文件路径"},
                    "old": {"type": "string", "description": "要替换的原文本，尽量包含足够上下文"},
                    "new": {"type": "string", "description": "替换后的文本"},
                    "replace_all": {"type": "boolean", "description": "是否替换所有匹配；默认 false"}
                },
                "required": ["path", "old", "new"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "mkdir",
            description: "在当前项目内创建目录",
            usage: "/mkdir notes",
            parameters: input_schema("要创建的项目内相对目录路径"),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "run_command",
            description: "在当前项目内运行安全白名单命令，例如 cargo check/fmt/test/build",
            usage: "/run_command cargo check",
            parameters: input_schema("要运行的白名单命令，例如 cargo check"),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "validate_project",
            description: "自动执行 cargo fmt、cargo check、cargo test；适合在修改 Rust 项目后做完整校验",
            usage: "/validate_project",
            parameters: empty_schema(),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "git_status",
            description: "查看当前项目的 Git 分支和工作区变更；只读，不会修改仓库",
            usage: "/git_status",
            parameters: empty_schema(),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "git_diff",
            description: "查看当前项目未暂存或已暂存的 Git 差异；只读，不会修改仓库",
            usage: "/git_diff {\"path\":\"src/main.rs\",\"staged\":false}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "可选的项目内相对路径；留空查看全部"},
                    "staged": {"type": "boolean", "description": "true 查看已暂存差异，默认 false"}
                }
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "web_search",
            description: "搜索网页；优先使用 Brave Search API，未配置密钥时自动改用 DuckDuckGo Lite",
            usage: "/web_search Rust async tutorial",
            parameters: input_schema("搜索关键词"),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "web_fetch",
            description: "读取 http/https 网页正文；适合配合 web_search 获取搜索结果详情",
            usage: "/web_fetch https://example.com",
            parameters: input_schema("要读取的 http 或 https URL"),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "rag_search",
            description: "当用户要求根据外部资料、知识库、笔记或 RAG 数据源回答时，检索用户添加的外部数据源",
            usage: "/rag_search Rust agent",
            parameters: input_schema("适合检索外部资料的关键词"),
            input_format: ToolInputFormat::PlainInput,
            source: ToolSource::Local,
        },
        Tool {
            name: "skill_load",
            description: "按需加载一个文件化 Skill；先根据名称和描述判断是否真的需要",
            usage: "/skill_load {\"name\":\"code-review\"}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "要加载的 Skill 名称"}
                },
                "required": ["name"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "skill_list",
            description: "列出当前工作区实际发现的全部 Skill；询问技能清单或数量时必须使用",
            usage: "/skill_list",
            parameters: empty_schema(),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "memory_add",
            description: "AI 自主保存稳定长期记忆，例如用户偏好、长期目标、项目事实；输入必须是 JSON",
            usage: "/memory_add {\"kind\":\"preference\",\"content\":\"用户偏好中文回答\"}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "description": "记忆类型，例如 preference、fact、goal"},
                    "content": {"type": "string", "description": "稳定偏好、长期目标或项目事实；不要包含密钥"}
                },
                "required": ["content"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "todo_add",
            description: "AI 自主添加一个或多个明确可执行的待办；输入必须是 JSON",
            usage: "/todo_add {\"titles\":[\"实现自动记忆\",\"更新文档\"]}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "单个待办标题"},
                    "titles": {
                        "type": "array",
                        "description": "多个待办标题",
                        "items": {"type": "string"}
                    }
                }
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "todo_update",
            description: "AI 自主全量更新当前 Todo 状态；用于拆解任务、设置进行中、完成或阻塞任务",
            usage: "/todo_update {\"todos\":[{\"id\":1,\"title\":\"任务\",\"status\":\"in_progress\"}]}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "description": "完整 Todo 列表，按执行顺序排列；同一时间最多一个 in_progress",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "integer", "description": "待办编号，从 1 开始"},
                                "title": {"type": "string", "description": "待办内容"},
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "done", "blocked"],
                                    "description": "待办状态"
                                }
                            },
                            "required": ["id", "title", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "todo_done",
            description: "AI 自主标记待办完成；输入必须是 JSON",
            usage: "/todo_done {\"id\":1}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "要标记完成的待办编号"}
                },
                "required": ["id"]
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "todo_list",
            description: "AI 自主查看当前待办列表",
            usage: "/todo_list",
            parameters: empty_schema(),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
        Tool {
            name: "dispatch_subagent",
            description: "派遣一个或多个独立子代理；researcher、planner、rust_teacher 等只读任务可以并行，包含命令执行的任务会按安全策略串行",
            usage: "/dispatch_subagent {\"tasks\":[{\"agent_type\":\"researcher\",\"task\":\"搜索 Rust async 资料\"},{\"agent_type\":\"planner\",\"task\":\"规划实现步骤\"}]}",
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_type": {
                        "type": "string",
                        "description": "子代理类型，例如 rust_teacher、reviewer、planner、researcher，也支持别名 teacher/review/plan/research"
                    },
                    "task": {
                        "type": "string",
                        "description": "交给子代理完成的独立任务，必须包含足够上下文"
                    },
                    "purpose": {
                        "type": "string",
                        "description": "为什么要派遣这个子代理，便于主 Agent 后续汇总"
                    },
                    "tasks": {
                        "type": "array",
                        "description": "多个相互独立的子任务；适合只读研究、规划、教学和资料整理",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent_type": {"type": "string"},
                                "task": {"type": "string"},
                                "purpose": {"type": "string"}
                            },
                            "required": ["agent_type", "task"]
                        }
                    }
                }
            }),
            input_format: ToolInputFormat::JsonObject,
            source: ToolSource::Local,
        },
    ]
}

pub async fn execute_tool(client: &Client, tool_name: &str, tool_input: &str) -> Result<String> {
    match tool_name {
        "echo" => Ok(run_echo(tool_input)),
        "calc" => calc::run(tool_input),
        "ls" => file::list_files(tool_input),
        "read" => file::read_file(tool_input),
        "read_lines" => file::read_lines(tool_input),
        "repo_map" => repo::repo_map(tool_input),
        "search_text" => search::search_text(tool_input),
        "write_file" => file::write_file(tool_input),
        "append_file" => file::append_file(tool_input),
        "replace_in_file" => file::replace_in_file(tool_input),
        "mkdir" => file::create_dir(tool_input),
        "run_command" => command::run(tool_input),
        "validate_project" => command::validate_project(),
        "git_status" => git::status(),
        "git_diff" => git::diff(tool_input),
        "web_search" => web::search(client, tool_input).await,
        "web_fetch" => web::fetch(client, tool_input).await,
        "rag_search" => rag_search(tool_input),
        "dispatch_subagent" => Err(anyhow!(
            "dispatch_subagent 是 Agent 内部调度工具，必须由 agent.rs 执行"
        )),
        _ => Err(anyhow!("工具 {tool_name} 已注册，但还没有实现")),
    }
}

pub fn tool_arguments_to_input(tool: &Tool, arguments: &str) -> Result<String> {
    match tool.input_format {
        ToolInputFormat::PlainInput => {
            if arguments.trim().is_empty() {
                return Ok(String::new());
            }

            let value: Value = serde_json::from_str(arguments)
                .context("工具参数必须是 JSON 对象，例如 {\"input\":\"src\"}")?;
            match value {
                Value::Object(map) => match map.get("input") {
                    Some(Value::String(input)) => Ok(input.clone()),
                    Some(other) => Ok(other.to_string()),
                    None => Ok(String::new()),
                },
                Value::String(input) => Ok(input),
                other => Ok(other.to_string()),
            }
        }
        ToolInputFormat::JsonObject => {
            if arguments.trim().is_empty() {
                return Ok("{}".to_string());
            }

            let value: Value =
                serde_json::from_str(arguments).context("工具参数必须是合法 JSON")?;
            if !value.is_object() {
                return Err(anyhow!("工具参数必须是 JSON 对象"));
            }

            Ok(value.to_string())
        }
    }
}

fn run_echo(input: &str) -> String {
    input.to_string()
}

fn rag_search(input: &str) -> Result<String> {
    let index = RetrievalIndex::load_default()?;
    Ok(index.format_search_results(input, 5))
}

fn input_schema(description: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "input": {
                "type": "string",
                "description": description
            }
        },
        "required": ["input"]
    })
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {}
    })
}
