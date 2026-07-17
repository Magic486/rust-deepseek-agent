use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

pub fn env_var(key: &str) -> Result<String> {
    if let Ok(value) = env::var(key) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Ok(value);
        }
    }

    for path in dotenv_candidates() {
        if let Some(value) = read_dotenv_value(&path, key)? {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Ok(value);
            }
        }
    }

    Err(anyhow!(
        "请设置环境变量 {key}，或在当前目录 .env / 用户配置目录 .rust-deepseek-agent/.env 中填写 {key}"
    ))
}

fn read_dotenv_value(path: &Path, key: &str) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("读取配置文件失败：{}", path.display()))?;

    Ok(parse_dotenv_content(&content, key))
}

fn parse_dotenv_content(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };

        if name.trim() == key {
            return Some(parse_dotenv_value(value.trim()));
        }
    }

    None
}

fn dotenv_candidates() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(".env")];
    let home = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"));
    if let Some(home) = home {
        paths.push(
            PathBuf::from(home)
                .join(".rust-deepseek-agent")
                .join(".env"),
        );
    }
    paths
}

fn parse_dotenv_value(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0] as char;
        let last = bytes[value.len() - 1] as char;

        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return value[1..value.len() - 1].to_string();
        }
    }

    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::parse_dotenv_content;

    #[test]
    fn parses_quoted_value_without_exposing_it() {
        let content = "# comment\nDEEPSEEK_API_KEY=\"test-value\"\n";
        assert_eq!(
            parse_dotenv_content(content, "DEEPSEEK_API_KEY").as_deref(),
            Some("test-value")
        );
    }
}
