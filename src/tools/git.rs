use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

const MAX_OUTPUT_CHARS: usize = 24_000;

#[derive(Deserialize)]
struct GitDiffInput {
    #[serde(default)]
    path: String,
    #[serde(default)]
    staged: bool,
}

pub fn status() -> Result<String> {
    run_git(&["status", "--short", "--branch"])
}

pub fn diff(input: &str) -> Result<String> {
    let request: GitDiffInput = serde_json::from_str(input).map_err(|error| {
        anyhow!(
            "git_diff 输入必须是 JSON：{{\"path\":\"src/main.rs\",\"staged\":false}}。解析错误：{error}"
        )
    })?;
    validate_relative_path(&request.path)?;

    let mut args = vec!["diff"];
    if request.staged {
        args.push("--cached");
    }
    if !request.path.trim().is_empty() {
        args.push("--");
        args.push(request.path.trim());
    }

    let output = run_git(&args)?;
    if output.trim().is_empty() {
        Ok("没有 Git 差异。".to_string())
    } else {
        Ok(output)
    }
}

fn run_git(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("执行 git {} 失败，请确认已安装 Git", args.join(" ")))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!(
            "Git 命令失败：{}",
            if error.is_empty() {
                output.status.to_string()
            } else {
                error
            }
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(truncate(stdout))
}

fn validate_relative_path(path: &str) -> Result<()> {
    if path.trim().is_empty() {
        return Ok(());
    }

    let path = Path::new(path.trim());
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(anyhow!("git_diff 的 path 只能是当前项目内的相对路径"));
    }
    Ok(())
}

fn truncate(output: String) -> String {
    if output.chars().count() <= MAX_OUTPUT_CHARS {
        return output;
    }
    let preview: String = output.chars().take(MAX_OUTPUT_CHARS).collect();
    format!("{preview}\n\n[Git 输出较长，只显示前 {MAX_OUTPUT_CHARS} 个字符]")
}

#[cfg(test)]
mod tests {
    use super::validate_relative_path;

    #[test]
    fn rejects_parent_path() {
        assert!(validate_relative_path("../outside.rs").is_err());
        assert!(validate_relative_path("src/main.rs").is_ok());
    }
}
