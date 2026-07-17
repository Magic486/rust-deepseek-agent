use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

const INSTRUCTION_FILES: &[&str] = &["AGENTS.md", "AGENT_GUIDE.md"];
const MAX_INSTRUCTION_CHARS: usize = 16_000;

pub struct WorkspaceContext {
    pub root: PathBuf,
    pub instructions: Option<String>,
}

impl WorkspaceContext {
    pub fn load() -> Result<Self> {
        let root = env::current_dir().context("获取当前工作目录失败")?;
        let instructions = load_instructions(&root)?;
        Ok(Self { root, instructions })
    }

    pub fn prompt_section(&self) -> String {
        let mut section = format!("当前工作目录：{}。", self.root.display());
        if let Some(instructions) = self.instructions.as_deref() {
            section
                .push_str("\n当前工作区提供了项目指令。执行文件修改、命令或代码审查时必须遵守：\n");
            section.push_str(instructions);
        }
        section
    }
}

fn load_instructions(root: &std::path::Path) -> Result<Option<String>> {
    for name in INSTRUCTION_FILES {
        let path = root.join(name);
        if !path.is_file() {
            continue;
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("读取工作区指令失败：{}", path.display()))?;
        let content = truncate(content);
        return Ok(Some(format!("来源：{name}\n\n{content}")));
    }
    Ok(None)
}

fn truncate(content: String) -> String {
    if content.chars().count() <= MAX_INSTRUCTION_CHARS {
        return content;
    }
    let preview: String = content.chars().take(MAX_INSTRUCTION_CHARS).collect();
    format!("{preview}\n\n[工作区指令较长，只加载前 {MAX_INSTRUCTION_CHARS} 个字符]")
}

#[cfg(test)]
mod tests {
    use super::WorkspaceContext;

    #[test]
    fn loads_current_workspace() {
        let workspace = WorkspaceContext::load().unwrap();
        assert!(workspace.root.is_dir());
        assert!(workspace.instructions.is_some());
    }
}
