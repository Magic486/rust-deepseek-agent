use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: usize,
    pub kind: String,
    pub content: String,
    pub created_at: u64,
}

pub struct MemoryStore {
    path: PathBuf,
    items: Vec<MemoryItem>,
}

impl MemoryStore {
    pub fn load_default() -> Result<Self> {
        let path = std::env::current_dir()?
            .join(".agent_data")
            .join("memory.json");
        Self::load(path)
    }

    fn load(path: PathBuf) -> Result<Self> {
        let items = if path.exists() {
            let content = fs::read_to_string(&path).context("读取 memory.json 失败")?;
            serde_json::from_str(&content).context("解析 memory.json 失败")?
        } else {
            Vec::new()
        };

        Ok(Self { path, items })
    }

    pub fn add(&mut self, kind: &str, content: &str) -> Result<MemoryItem> {
        let item = MemoryItem {
            id: self.next_id(),
            kind: kind.to_string(),
            content: content.to_string(),
            created_at: now_seconds(),
        };

        self.items.push(item.clone());
        self.save()?;
        Ok(item)
    }

    pub fn list(&self) -> String {
        if self.items.is_empty() {
            return "还没有长期记忆。".to_string();
        }

        self.items
            .iter()
            .map(|item| format!("#{} [{}] {}", item.id, item.kind, item.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn search(&self, keyword: &str) -> String {
        let keyword = keyword.to_lowercase();
        let results: Vec<String> = self
            .items
            .iter()
            .filter(|item| item.content.to_lowercase().contains(&keyword))
            .map(|item| format!("#{} [{}] {}", item.id, item.kind, item.content))
            .collect();

        if results.is_empty() {
            "没有找到相关记忆。".to_string()
        } else {
            results.join("\n")
        }
    }

    pub fn enrich_user_input(&self, user_input: &str) -> String {
        let related = self.related_items(user_input);

        if related.is_empty() {
            return user_input.to_string();
        }

        format!(
            "{user_input}\n\n相关长期记忆：\n{}",
            related
                .iter()
                .map(|item| format!("- {}", item.content))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    fn related_items(&self, user_input: &str) -> Vec<&MemoryItem> {
        let words: Vec<String> = user_input
            .split_whitespace()
            .map(|word| word.to_lowercase())
            .filter(|word| word.chars().count() >= 2)
            .collect();

        self.items
            .iter()
            .filter(|item| {
                let content = item.content.to_lowercase();
                words.iter().any(|word| content.contains(word))
            })
            .take(5)
            .collect()
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("创建 .agent_data 目录失败")?;
        }

        let content = serde_json::to_string_pretty(&self.items)?;
        fs::write(&self.path, content).context("写入 memory.json 失败")
    }

    fn next_id(&self) -> usize {
        self.items.iter().map(|item| item.id).max().unwrap_or(0) + 1
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
