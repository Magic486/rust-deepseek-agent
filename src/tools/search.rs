use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

const MAX_FILES: usize = 2_000;
const MAX_MATCHES: usize = 200;
const MAX_FILE_BYTES: u64 = 1_000_000;

#[derive(Deserialize)]
struct SearchInput {
    query: String,
    #[serde(default = "default_path")]
    path: String,
    #[serde(default)]
    case_sensitive: bool,
}

pub fn search_text(input: &str) -> Result<String> {
    let request: SearchInput = serde_json::from_str(input).map_err(|error| {
        anyhow!(
            "search_text 输入必须是 JSON：{{\"query\":\"关键词\",\"path\":\"src\",\"case_sensitive\":false}}。解析错误：{error}"
        )
    })?;

    if request.query.trim().is_empty() {
        return Err(anyhow!("query 不能为空"));
    }

    let base = env::current_dir()
        .context("获取当前项目目录失败")?
        .canonicalize()
        .context("解析当前项目目录失败")?;
    let target = resolve_project_path(&base, &request.path)?;
    let mut files = Vec::new();
    collect_files(&target, &mut files)?;

    let query = if request.case_sensitive {
        request.query.clone()
    } else {
        request.query.to_lowercase()
    };
    let mut matches = Vec::new();
    let mut scanned = 0usize;

    for file in files.into_iter().take(MAX_FILES) {
        let metadata = match fs::metadata(&file) {
            Ok(metadata) if metadata.len() <= MAX_FILE_BYTES => metadata,
            _ => continue,
        };
        if !metadata.is_file() {
            continue;
        }

        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        scanned += 1;

        for (line_index, line) in content.lines().enumerate() {
            let searchable = if request.case_sensitive {
                line.to_string()
            } else {
                line.to_lowercase()
            };
            if searchable.contains(&query) {
                let relative = file.strip_prefix(&base).unwrap_or(&file);
                matches.push(format!(
                    "{}:{}: {}",
                    relative.display(),
                    line_index + 1,
                    line.trim()
                ));
                if matches.len() >= MAX_MATCHES {
                    break;
                }
            }
        }

        if matches.len() >= MAX_MATCHES {
            break;
        }
    }

    if matches.is_empty() {
        Ok(format!(
            "没有找到 `{}`。已扫描 {scanned} 个文本文件。",
            request.query
        ))
    } else {
        let limited = matches.len() == MAX_MATCHES;
        let mut output = format!(
            "找到 {} 处匹配（扫描 {scanned} 个文本文件）：\n{}",
            matches.len(),
            matches.join("\n")
        );
        if limited {
            output.push_str("\n\n[匹配较多，只显示前 200 处]");
        }
        Ok(output)
    }
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }

    for entry in fs::read_dir(path).with_context(|| format!("读取目录失败：{}", path.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if should_skip_dir(&entry.file_name().to_string_lossy()) {
                continue;
            }
            collect_files(&entry_path, files)?;
        } else if file_type.is_file() {
            files.push(entry_path);
        }

        if files.len() >= MAX_FILES {
            break;
        }
    }

    Ok(())
}

fn resolve_project_path(base: &Path, input: &str) -> Result<PathBuf> {
    let relative = Path::new(input.trim());
    if relative.is_absolute() {
        return Err(anyhow!("只允许搜索当前项目内的相对路径"));
    }

    let target = base
        .join(relative)
        .canonicalize()
        .context("搜索路径不存在")?;
    if !target.starts_with(base) {
        return Err(anyhow!("只允许搜索当前项目目录里面的文件"));
    }
    Ok(target)
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".agent_data"
            | "target"
            | "node_modules"
            | ".next"
            | "dist"
            | "build"
            | ".venv"
            | "__pycache__"
    )
}

fn default_path() -> String {
    ".".to_string()
}

#[cfg(test)]
mod tests {
    use super::search_text;

    #[test]
    fn searches_project_text() {
        let output =
            search_text(r#"{"query":"fn searches_project_text","path":"src/tools/search.rs"}"#)
                .unwrap();
        assert!(output.contains("src\\tools\\search.rs") || output.contains("src/tools/search.rs"));
    }
}
