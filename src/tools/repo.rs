use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

const MAX_FILES: usize = 160;
const MAX_SYMBOLS_PER_FILE: usize = 16;
const MAX_OUTPUT_CHARS: usize = 24_000;

pub fn repo_map(input: &str) -> Result<String> {
    let base_dir = current_project_dir()?;
    let root = resolve_project_path(&base_dir, input)?;

    if !root.is_dir() {
        return Ok("repo_map 目标必须是项目内目录".to_string());
    }

    let mut files = Vec::new();
    collect_files(&root, &base_dir, &mut files)?;
    files.sort();

    let total_files = files.len();
    let shown_files: Vec<PathBuf> = files.into_iter().take(MAX_FILES).collect();

    let mut output = String::new();
    output.push_str("# Repo Map\n\n");
    output.push_str(&format!(
        "根目录：{}\n",
        display_project_path(&base_dir, &root)
    ));
    output.push_str(&format!("扫描文件数：{total_files}\n"));
    if total_files > MAX_FILES {
        output.push_str(&format!("展示上限：前 {MAX_FILES} 个文件\n"));
    }

    output.push_str("\n## 文件结构\n\n");
    for file in &shown_files {
        output.push_str("- ");
        output.push_str(&display_project_path(&base_dir, file));
        output.push('\n');
    }

    let mut symbol_sections = Vec::new();
    for file in &shown_files {
        if file.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }

        let symbols = rust_symbols(file)?;
        if symbols.is_empty() {
            continue;
        }

        let mut section = String::new();
        section.push_str(&format!(
            "\n### {}\n\n",
            display_project_path(&base_dir, file)
        ));
        for symbol in symbols {
            section.push_str("- ");
            section.push_str(&symbol);
            section.push('\n');
        }
        symbol_sections.push(section);
    }

    if !symbol_sections.is_empty() {
        output.push_str("\n## Rust 符号\n");
        for section in symbol_sections {
            output.push_str(&section);
        }
    }

    Ok(truncate_output(output))
}

fn collect_files(dir: &Path, base_dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if files.len() >= MAX_FILES {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("读取目录失败：{}", display_project_path(base_dir, dir)))?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        if files.len() >= MAX_FILES {
            break;
        }

        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if should_skip_dir(&file_name) {
                continue;
            }
            collect_files(&path, base_dir, files)?;
        } else if file_type.is_file() && should_include_file(&path) {
            files.push(path);
        }
    }

    Ok(())
}

fn rust_symbols(file: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(file).context("读取 Rust 文件失败")?;
    let mut symbols = Vec::new();

    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(symbol) = parse_rust_symbol(trimmed) {
            symbols.push(format!("L{} {symbol}", index + 1));
        }

        if symbols.len() >= MAX_SYMBOLS_PER_FILE {
            symbols.push("... 符号较多，已截断".to_string());
            break;
        }
    }

    Ok(symbols)
}

fn parse_rust_symbol(line: &str) -> Option<String> {
    let prefixes = [
        "pub struct ",
        "struct ",
        "pub enum ",
        "enum ",
        "pub trait ",
        "trait ",
        "pub fn ",
        "fn ",
        "pub async fn ",
        "async fn ",
        "impl ",
        "pub mod ",
        "mod ",
    ];

    if !prefixes.iter().any(|prefix| line.starts_with(prefix)) {
        return None;
    }

    let signature = line
        .split('{')
        .next()
        .unwrap_or(line)
        .split(';')
        .next()
        .unwrap_or(line)
        .trim();

    Some(signature.to_string())
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

fn should_include_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if name.starts_with('.') && name != ".gitignore" && name != ".env.example" {
        return false;
    }

    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("rs" | "toml" | "md" | "json" | "yaml" | "yml" | "txt" | "html" | "css" | "js" | "ts")
    )
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
        return Err(anyhow!("只允许扫描当前项目内的相对路径"));
    }

    let target = base_dir.join(relative_path);
    let target = target.canonicalize().context("路径不存在或无法访问")?;

    if !target.starts_with(base_dir) {
        return Err(anyhow!("只允许扫描当前项目目录里面的内容"));
    }

    Ok(target)
}

fn display_project_path(base_dir: &Path, path: &Path) -> String {
    path.strip_prefix(base_dir)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn truncate_output(output: String) -> String {
    if output.chars().count() <= MAX_OUTPUT_CHARS {
        return output;
    }

    let preview: String = output.chars().take(MAX_OUTPUT_CHARS).collect();
    format!("{preview}\n\n[Repo Map 较长，只显示前 {MAX_OUTPUT_CHARS} 个字符]")
}

#[cfg(test)]
mod tests {
    use super::repo_map;

    #[test]
    fn repo_map_scans_src_directory() {
        let output = repo_map("src").unwrap();

        assert!(output.contains("# Repo Map"));
        assert!(output.contains("src"));
        assert!(output.contains("Rust 符号"));
    }
}
