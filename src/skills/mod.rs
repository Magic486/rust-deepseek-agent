use anyhow::{Result, anyhow};

pub struct Skill {
    pub name: &'static str,
    pub description: &'static str,
    pub prompt: &'static str,
}

pub struct SkillRegistry {
    skills: Vec<Skill>,
    active_skill: Option<String>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: vec![
                Skill {
                    name: "rust_teacher",
                    description: "用新手友好的方式解释 Rust 代码和概念",
                    prompt: "你现在启用了 rust_teacher 技能。解释 Rust 时要先讲直觉，再讲语法，最后给一个小例子。",
                },
                Skill {
                    name: "code_reviewer",
                    description: "从 bug、可读性、边界条件角度审查代码",
                    prompt: "你现在启用了 code_reviewer 技能。回答时优先指出具体风险、文件位置和可改进点。",
                },
                Skill {
                    name: "planner",
                    description: "把目标拆成清晰、可执行的小步骤",
                    prompt: "你现在启用了 planner 技能。先拆目标，再标注优先级，保持步骤短小明确。",
                },
            ],
            active_skill: None,
        }
    }

    pub fn list(&self) -> String {
        let mut lines = Vec::new();

        for skill in &self.skills {
            let active = if self.active_skill.as_deref() == Some(skill.name) {
                "（当前启用）"
            } else {
                ""
            };
            lines.push(format!("- {}{}：{}", skill.name, active, skill.description));
        }

        lines.join("\n")
    }

    pub fn activate(&mut self, name: &str) -> Result<String> {
        if self.skills.iter().any(|skill| skill.name == name) {
            self.active_skill = Some(name.to_string());
            Ok(format!("已启用技能：{name}"))
        } else {
            Err(anyhow!("没有找到技能：{name}"))
        }
    }

    pub fn active_prompt(&self) -> Option<&str> {
        let active_name = self.active_skill.as_deref()?;
        self.skills
            .iter()
            .find(|skill| skill.name == active_name)
            .map(|skill| skill.prompt)
    }
}
