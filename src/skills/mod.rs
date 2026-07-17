use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

const MAX_SKILL_CHARS: usize = 32_000;

#[derive(Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub source: PathBuf,
}

#[derive(Clone)]
pub struct SkillSnapshotItem {
    pub name: String,
    pub loaded: bool,
}

#[derive(Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

pub struct SkillRegistry {
    skills: Vec<Skill>,
    loaded: HashSet<String>,
}

impl SkillRegistry {
    pub fn discover(workspace_root: &Path) -> Result<Self> {
        let mut roots = vec![workspace_root.join(".agents").join("skills")];
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            roots.push(PathBuf::from(home).join(".agents").join("skills"));
        }
        Self::discover_from_roots(&roots)
    }

    fn discover_from_roots(roots: &[PathBuf]) -> Result<Self> {
        let mut skills = Vec::new();
        let mut names = HashSet::new();

        for root in roots {
            if !root.is_dir() {
                continue;
            }
            for entry in fs::read_dir(root)
                .with_context(|| format!("读取 Skill 目录失败：{}", root.display()))?
            {
                let entry = entry?;
                let path = entry.path().join("SKILL.md");
                if !path.is_file() {
                    continue;
                }
                let skill = parse_skill(&path)?;
                if !names.insert(skill.name.clone()) {
                    continue;
                }
                skills.push(skill);
            }
        }

        skills.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Self {
            skills,
            loaded: HashSet::new(),
        })
    }

    pub fn list(&self) -> String {
        if self.skills.is_empty() {
            return "没有发现 Skill。请在 .agents/skills/<name>/SKILL.md 中添加。".to_string();
        }

        self.skills
            .iter()
            .map(|skill| {
                let state = if self.loaded.contains(&skill.name) {
                    "（已加载）"
                } else {
                    ""
                };
                format!("- {}{}：{}", skill.name, state, skill.description)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn catalog_for_prompt(&self) -> String {
        if self.skills.is_empty() {
            return "（没有发现可用 Skill）".to_string();
        }
        self.skills
            .iter()
            .map(|skill| format!("- {}：{}", skill.name, skill.description))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn load(&mut self, name: &str) -> Result<String> {
        let name = name.trim();
        let skill = self
            .skills
            .iter()
            .find(|skill| skill.name == name)
            .ok_or_else(|| anyhow!("没有找到 Skill：{name}"))?;
        self.loaded.insert(skill.name.clone());
        Ok(format!(
            "已加载 Skill `{}`，后续工作必须遵守以下说明。\n来源：{}\n\n{}",
            skill.name,
            skill.source.display(),
            skill.instructions
        ))
    }

    pub fn reset_loaded(&mut self) {
        self.loaded.clear();
    }

    pub fn snapshot(&self) -> Vec<SkillSnapshotItem> {
        self.skills
            .iter()
            .map(|skill| SkillSnapshotItem {
                name: skill.name.clone(),
                loaded: self.loaded.contains(&skill.name),
            })
            .collect()
    }
}

fn parse_skill(path: &Path) -> Result<Skill> {
    let content =
        fs::read_to_string(path).with_context(|| format!("读取 Skill 失败：{}", path.display()))?;
    let rest = content
        .strip_prefix("---")
        .ok_or_else(|| anyhow!("Skill 缺少 YAML frontmatter：{}", path.display()))?;
    let (frontmatter, body) = rest
        .split_once("\n---")
        .ok_or_else(|| anyhow!("Skill frontmatter 没有结束标记：{}", path.display()))?;
    let metadata: SkillFrontmatter = serde_yaml::from_str(frontmatter)
        .with_context(|| format!("解析 Skill frontmatter 失败：{}", path.display()))?;

    validate_name(&metadata.name)?;
    let directory_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if directory_name != metadata.name {
        return Err(anyhow!(
            "Skill 名称 `{}` 必须和目录名 `{directory_name}` 一致",
            metadata.name
        ));
    }
    let description = metadata.description.trim().to_string();
    if description.is_empty() || description.chars().count() > 1024 {
        return Err(anyhow!("Skill description 必须为 1 到 1024 个字符"));
    }
    let instructions = body.trim_start_matches(['\r', '\n']).trim().to_string();
    if instructions.is_empty() {
        return Err(anyhow!("Skill 正文不能为空：{}", path.display()));
    }
    if instructions.chars().count() > MAX_SKILL_CHARS {
        return Err(anyhow!(
            "Skill 正文超过 {MAX_SKILL_CHARS} 个字符：{}",
            path.display()
        ));
    }

    Ok(Skill {
        name: metadata.name,
        description,
        instructions,
        source: path.to_path_buf(),
    })
}

fn validate_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
    if valid {
        Ok(())
    } else {
        Err(anyhow!("Skill 名称不合法：{name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::SkillRegistry;
    use std::fs;

    #[test]
    fn discovers_and_loads_file_skill() {
        let root = std::env::temp_dir().join(format!("agent-skill-test-{}", std::process::id()));
        let skill_dir = root.join("demo-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo skill\n---\n\nFollow the demo workflow.",
        )
        .unwrap();

        let mut registry = SkillRegistry::discover_from_roots(std::slice::from_ref(&root)).unwrap();
        assert!(registry.list().contains("demo-skill"));
        assert!(
            registry
                .load("demo-skill")
                .unwrap()
                .contains("Follow the demo workflow")
        );
        assert!(registry.snapshot()[0].loaded);

        let _ = fs::remove_dir_all(root);
    }
}
