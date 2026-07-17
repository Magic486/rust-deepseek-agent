use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

#[derive(Deserialize)]
struct WriteFileInput {
    path: String,
    content: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
struct AppendFileInput {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct ReplaceFileInput {
    path: String,
    old: String,
    new: String,
    #[serde(default)]
    replace_all: bool,
}

#[derive(Deserialize)]
struct ReadLinesInput {
    path: String,
    #[serde(default = "default_start_line")]
    start_line: usize,
    #[serde(default = "default_line_count")]
    line_count: usize,
}

pub fn list_files(input: &str) -> Result<String> {
    let base_dir = current_project_dir()?;
    let target_dir = resolve_project_path(&base_dir, input)?;

    if !target_dir.is_dir() {
        return Ok("目标不是文件夹".to_string());
    }

    let mut lines = Vec::new();
    for entry in fs::read_dir(&target_dir).context("读取文件夹失败")? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            lines.push(format!("[目录] {file_name}"));
        } else {
            lines.push(format!("[文件] {file_name}"));
        }
    }

    lines.sort();

    if lines.is_empty() {
        Ok("这个文件夹是空的".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

pub fn read_file(input: &str) -> Result<String> {
    if input.trim().is_empty() {
        return Ok("格式是：/read 文件路径，比如 /read src/main.rs".to_string());
    }

    let base_dir = current_project_dir()?;
    let target_file = resolve_project_path(&base_dir, input)?;

    if !target_file.is_file() {
        return Ok("目标不是文本文件".to_string());
    }

    let content =
        fs::read_to_string(&target_file).context("读取文件失败，可能不是 UTF-8 文本文件")?;

    if content.chars().count() > 8_000 {
        let preview: String = content.chars().take(8_000).collect();
        Ok(format!("{}\n\n[内容较长，只显示前 8000 个字符]", preview))
    } else {
        Ok(content)
    }
}

pub fn read_lines(input: &str) -> Result<String> {
    let request: ReadLinesInput = serde_json::from_str(input).map_err(|error| {
        anyhow!(
            "read_lines 输入必须是 JSON：{{\"path\":\"src/main.rs\",\"start_line\":1,\"line_count\":200}}。解析错误：{error}"
        )
    })?;

    if request.path.trim().is_empty() {
        return Err(anyhow!("path 不能为空"));
    }
    if request.start_line == 0 {
        return Err(anyhow!("start_line 从 1 开始"));
    }
    if request.line_count == 0 || request.line_count > 400 {
        return Err(anyhow!("line_count 必须在 1 到 400 之间"));
    }

    let base_dir = current_project_dir()?;
    let target_file = resolve_project_path(&base_dir, &request.path)?;
    if !target_file.is_file() {
        return Err(anyhow!("目标不是文本文件"));
    }

    let content =
        fs::read_to_string(&target_file).context("读取文件失败，可能不是 UTF-8 文本文件")?;
    let lines: Vec<&str> = content.lines().collect();
    if request.start_line > lines.len().max(1) {
        return Ok(format!(
            "起始行 {} 超出文件范围；文件共 {} 行。",
            request.start_line,
            lines.len()
        ));
    }

    let start_index = request.start_line - 1;
    let end_index = (start_index + request.line_count).min(lines.len());
    let width = end_index.to_string().len().max(1);
    let body = lines[start_index..end_index]
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            format!(
                "{:>width$} | {}",
                request.start_line + offset,
                line,
                width = width
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        "{}（第 {}-{} 行，共 {} 行）\n{}",
        display_project_path(&base_dir, &target_file),
        request.start_line,
        end_index,
        lines.len(),
        body
    ))
}

pub fn write_file(input: &str) -> Result<String> {
    let request: WriteFileInput = serde_json::from_str(input).map_err(|error| {
        anyhow!(
            "write_file 输入必须是 JSON：{{\"path\":\"文件路径\",\"content\":\"内容\",\"overwrite\":false}}。解析错误：{error}"
        )
    })?;

    if request.path.trim().is_empty() {
        return Ok("path 不能为空".to_string());
    }

    let base_dir = current_project_dir()?;
    let target_file = resolve_writable_project_path(&base_dir, &request.path)?;

    if target_file.exists() && !request.overwrite {
        return Ok("文件已存在。如需覆盖，请把 overwrite 设为 true。".to_string());
    }

    fs::write(&target_file, request.content).context("写入文件失败")?;
    Ok(format!(
        "已写入文件：{}",
        display_project_path(&base_dir, &target_file)
    ))
}

pub fn append_file(input: &str) -> Result<String> {
    let request: AppendFileInput = serde_json::from_str(input).map_err(|error| {
        anyhow!(
            "append_file 输入必须是 JSON：{{\"path\":\"文件路径\",\"content\":\"追加内容\"}}。解析错误：{error}"
        )
    })?;

    if request.path.trim().is_empty() {
        return Ok("path 不能为空".to_string());
    }

    let base_dir = current_project_dir()?;
    let target_file = resolve_writable_project_path(&base_dir, &request.path)?;

    if !target_file.exists() {
        return Ok("文件不存在。请先用 write_file 创建文件。".to_string());
    }

    if !target_file.is_file() {
        return Ok("目标不是文件".to_string());
    }

    let mut content = fs::read_to_string(&target_file).context("读取原文件失败")?;
    content.push_str(&request.content);
    fs::write(&target_file, content).context("追加写入失败")?;

    Ok(format!(
        "已追加内容到：{}",
        display_project_path(&base_dir, &target_file)
    ))
}

pub fn replace_in_file(input: &str) -> Result<String> {
    let request: ReplaceFileInput = serde_json::from_str(input).map_err(|error| {
        anyhow!(
            "replace_in_file 输入必须是 JSON：{{\"path\":\"文件路径\",\"old\":\"旧内容\",\"new\":\"新内容\",\"replace_all\":false}}。解析错误：{error}"
        )
    })?;

    if request.path.trim().is_empty() {
        return Ok("path 不能为空".to_string());
    }

    if request.old.is_empty() {
        return Ok("old 不能为空".to_string());
    }

    let base_dir = current_project_dir()?;
    let target_file = resolve_project_path(&base_dir, &request.path)?;

    if !target_file.is_file() {
        return Ok("目标不是文件".to_string());
    }

    let content = fs::read_to_string(&target_file).context("读取文件失败")?;
    let match_count = content.matches(&request.old).count();

    if match_count == 0 {
        return Ok("没有找到要替换的内容。".to_string());
    }

    if match_count > 1 && !request.replace_all {
        return Ok(format!(
            "找到 {match_count} 处匹配。为避免误改，请把 replace_all 设为 true，或提供更精确的 old 内容。"
        ));
    }

    let updated = if request.replace_all {
        content.replace(&request.old, &request.new)
    } else {
        content.replacen(&request.old, &request.new, 1)
    };

    fs::write(&target_file, updated).context("写回文件失败")?;

    Ok(format!(
        "已替换 {match_count} 处内容：{}",
        display_project_path(&base_dir, &target_file)
    ))
}

pub fn create_dir(input: &str) -> Result<String> {
    if input.trim().is_empty() {
        return Ok("格式是：/mkdir 相对目录，比如 /mkdir notes".to_string());
    }

    let base_dir = current_project_dir()?;
    let target_dir = resolve_creatable_project_path(&base_dir, input)?;
    fs::create_dir_all(&target_dir).context("创建目录失败")?;

    Ok(format!(
        "已创建目录：{}",
        display_project_path(&base_dir, &target_dir)
    ))
}

fn current_project_dir() -> Result<PathBuf> {
    env::current_dir()
        .context("获取当前项目目录失败")?
        .canonicalize()
        .context("解析当前项目目录失败")
}

fn resolve_project_path(base_dir: &Path, input: &str) -> Result<PathBuf> {
    let relative_path = if input.trim().is_empty() {
        Path::new(".")
    } else {
        Path::new(input.trim())
    };

    if relative_path.is_absolute() {
        return Err(anyhow!("只允许访问当前项目内的相对路径"));
    }

    let target = base_dir.join(relative_path);
    let target = target.canonicalize().context("路径不存在或无法访问")?;

    if !target.starts_with(base_dir) {
        return Err(anyhow!("只允许访问当前项目目录里面的文件"));
    }

    Ok(target)
}

fn resolve_writable_project_path(base_dir: &Path, input: &str) -> Result<PathBuf> {
    let relative_path = Path::new(input.trim());

    if relative_path.as_os_str().is_empty() {
        return Err(anyhow!("路径不能为空"));
    }

    if relative_path.is_absolute() {
        return Err(anyhow!("只允许写入当前项目内的相对路径"));
    }

    if relative_path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!("路径不能包含 .."));
    }

    let target = base_dir.join(relative_path);

    if target.exists() {
        let metadata = fs::symlink_metadata(&target).context("读取目标路径元数据失败")?;
        if metadata.file_type().is_symlink() {
            return Err(anyhow!("为避免写到项目外，文件工具不允许写入符号链接"));
        }

        let canonical_target = target.canonicalize().context("解析目标路径失败")?;
        if !canonical_target.starts_with(base_dir) {
            return Err(anyhow!("只允许写入当前项目目录里面的文件"));
        }
    }

    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("无法解析目标路径的父目录"))?;
    let parent = parent
        .canonicalize()
        .context("目标父目录不存在或无法访问")?;

    if !parent.starts_with(base_dir) {
        return Err(anyhow!("只允许写入当前项目目录里面的文件"));
    }

    Ok(target)
}

fn resolve_creatable_project_path(base_dir: &Path, input: &str) -> Result<PathBuf> {
    let relative_path = Path::new(input.trim());

    if relative_path.as_os_str().is_empty() {
        return Err(anyhow!("路径不能为空"));
    }

    if relative_path.is_absolute() {
        return Err(anyhow!("只允许创建当前项目内的相对路径"));
    }

    if relative_path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!("路径不能包含 .."));
    }

    let target = base_dir.join(relative_path);
    let mut ancestor = target.as_path();

    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| anyhow!("无法解析目标路径的上级目录"))?;
    }

    let ancestor = ancestor.canonicalize().context("解析上级目录失败")?;
    if !ancestor.starts_with(base_dir) {
        return Err(anyhow!("只允许创建当前项目目录里面的目录"));
    }

    Ok(target)
}

fn display_project_path(base_dir: &Path, path: &Path) -> String {
    path.strip_prefix(base_dir)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn default_start_line() -> usize {
    1
}

fn default_line_count() -> usize {
    200
}

#[cfg(test)]
mod tests {
    use super::read_lines;

    #[test]
    fn reads_a_line_range_with_numbers() {
        let output = read_lines(r#"{"path":"src/main.rs","start_line":1,"line_count":3}"#).unwrap();
        assert!(output.contains("1 | mod agent;"));
        assert!(output.contains("第 1-3 行"));
    }

    #[test]
    fn rejects_too_many_lines() {
        let result = read_lines(r#"{"path":"src/main.rs","start_line":1,"line_count":401}"#);
        assert!(result.is_err());
    }
}
