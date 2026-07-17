use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct RagSource {
    pub id: usize,
    pub kind: String,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone)]
pub struct DocumentChunk {
    pub source_name: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
}

pub struct SearchResult {
    pub score: usize,
    pub chunk: DocumentChunk,
}

pub struct RetrievalIndex {
    config_path: PathBuf,
    sources: Vec<RagSource>,
    chunks: Vec<DocumentChunk>,
}

impl RetrievalIndex {
    pub fn load_default() -> Result<Self> {
        let config_path = default_config_path()?;
        Self::load(config_path)
    }

    fn load(config_path: PathBuf) -> Result<Self> {
        let sources = load_sources(&config_path)?;
        let chunks = build_chunks(&sources)?;

        Ok(Self {
            config_path,
            sources,
            chunks,
        })
    }

    pub fn reindex(&mut self) -> Result<()> {
        self.chunks = build_chunks(&self.sources)?;
        Ok(())
    }

    pub fn add_folder(&mut self, name: &str, path: &str) -> Result<RagSource> {
        let path = PathBuf::from(path.trim());
        if !path.exists() {
            return Err(anyhow!("RAG 数据源路径不存在：{}", path.display()));
        }
        if !path.is_dir() {
            return Err(anyhow!("RAG 数据源必须是文件夹：{}", path.display()));
        }

        let source = RagSource {
            id: self.next_id(),
            kind: "folder".to_string(),
            name: name.trim().to_string(),
            path,
        };

        if source.name.is_empty() {
            return Err(anyhow!("RAG 数据源名称不能为空"));
        }

        self.sources.push(source.clone());
        self.save_sources()?;
        self.reindex()?;
        Ok(source)
    }

    pub fn remove(&mut self, id: usize) -> Result<String> {
        let Some(index) = self.sources.iter().position(|source| source.id == id) else {
            return Err(anyhow!("没有找到 RAG 数据源 #{id}"));
        };

        let source = self.sources.remove(index);
        self.save_sources()?;
        self.reindex()?;
        Ok(format!("已删除 RAG 数据源 #{}：{}", source.id, source.name))
    }

    pub fn list_sources(&self) -> String {
        if self.sources.is_empty() {
            return format!(
                "还没有 RAG 数据源。\n使用：/rag add-folder 名称 路径\n配置文件：{}",
                self.config_path.display()
            );
        }

        self.sources
            .iter()
            .map(|source| {
                format!(
                    "#{} [{}] {} -> {}",
                    source.id,
                    source.kind,
                    source.name,
                    source.path.display()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let keywords = keywords(query);
        if keywords.is_empty() {
            return Vec::new();
        }

        let mut results: Vec<SearchResult> = self
            .chunks
            .iter()
            .filter_map(|chunk| {
                let score = score_chunk(chunk, &keywords);
                (score > 0).then(|| SearchResult {
                    score,
                    chunk: chunk.clone(),
                })
            })
            .collect();

        results.sort_by(|a, b| b.score.cmp(&a.score));
        results.truncate(limit);
        results
    }

    pub fn format_search_results(&self, query: &str, limit: usize) -> String {
        if self.sources.is_empty() {
            return "还没有 RAG 数据源。请先使用 /rag add-folder 名称 路径。".to_string();
        }

        let results = self.search(query, limit);
        if results.is_empty() {
            return "没有在 RAG 数据源中找到相关资料。".to_string();
        }

        let mut output = String::from("RAG 检索结果：");
        for result in results {
            output.push_str(&format!(
                "\n\n[{} | {}:{}-{} | score {}]\n{}",
                result.chunk.source_name,
                result.chunk.path,
                result.chunk.start_line,
                result.chunk.end_line,
                result.score,
                result.chunk.text
            ));
        }
        output
    }

    fn save_sources(&self) -> Result<()> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).context("创建 .agent_data 目录失败")?;
        }

        let content = serde_json::to_string_pretty(&self.sources)?;
        fs::write(&self.config_path, content).context("写入 rag_sources.json 失败")
    }

    fn next_id(&self) -> usize {
        self.sources
            .iter()
            .map(|source| source.id)
            .max()
            .unwrap_or(0)
            + 1
    }
}

fn load_sources(path: &Path) -> Result<Vec<RagSource>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(path).context("读取 rag_sources.json 失败")?;
    serde_json::from_str(&content).context("解析 rag_sources.json 失败")
}

fn build_chunks(sources: &[RagSource]) -> Result<Vec<DocumentChunk>> {
    let mut chunks = Vec::new();

    for source in sources {
        if source.kind != "folder" {
            continue;
        }
        let mut files = Vec::new();
        collect_files(&source.path, &mut files)?;

        for file in files {
            chunks.extend(chunks_from_file(source, &file)?);
        }
    }

    Ok(chunks)
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("读取目录失败：{}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if should_skip(&name) {
            continue;
        }

        if path.is_dir() {
            collect_files(&path, files)?;
        } else if is_indexable_file(&path)? {
            files.push(path);
        }
    }

    Ok(())
}

fn should_skip(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "target" | "node_modules" | "dist" | "build" | ".agent_data"
        )
}

fn is_indexable_file(path: &Path) -> Result<bool> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_FILE_BYTES {
        return Ok(false);
    }

    Ok(matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some(
            "md" | "txt"
                | "rs"
                | "toml"
                | "json"
                | "yaml"
                | "yml"
                | "py"
                | "js"
                | "ts"
                | "tsx"
                | "jsx"
                | "html"
                | "css"
        )
    ))
}

fn chunks_from_file(source: &RagSource, path: &Path) -> Result<Vec<DocumentChunk>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("读取 RAG 文件失败：{}", path.display()))?;
    let relative = path
        .strip_prefix(&source.path)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks = Vec::new();

    let chunk_size = 40;
    let overlap = 8;
    let mut start = 0;

    while start < lines.len() {
        let end = (start + chunk_size).min(lines.len());
        let text = lines[start..end].join("\n");

        if !text.trim().is_empty() {
            chunks.push(DocumentChunk {
                source_name: source.name.clone(),
                path: relative.clone(),
                start_line: start + 1,
                end_line: end,
                text,
            });
        }

        if end == lines.len() {
            break;
        }
        start = end.saturating_sub(overlap);
    }

    Ok(chunks)
}

fn score_chunk(chunk: &DocumentChunk, keywords: &[String]) -> usize {
    let path = chunk.path.to_lowercase();
    let text = chunk.text.to_lowercase();

    keywords
        .iter()
        .map(|keyword| {
            let mut score = text.matches(keyword).count();
            if path.contains(keyword) {
                score += 3;
            }
            score
        })
        .sum()
}

fn keywords(query: &str) -> Vec<String> {
    query
        .split(|ch: char| ch.is_whitespace() || ch.is_ascii_punctuation())
        .map(str::trim)
        .filter(|word| word.chars().count() >= 2)
        .map(|word| word.to_lowercase())
        .collect()
}

fn default_config_path() -> Result<PathBuf> {
    Ok(std::env::current_dir()?
        .join(".agent_data")
        .join("rag_sources.json"))
}
