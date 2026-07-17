mod agent;
mod config;
mod hooks;
mod llm;
mod mcp;
mod memory;
mod message;
mod retrieval;
mod skills;
mod sub_agent;
mod todo;
mod tools;
mod ui;
mod workspace;

use anyhow::{Context, Result};

use crate::agent::Agent;

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = config::env_var("DEEPSEEK_API_KEY")
        .context("请配置 DEEPSEEK_API_KEY 后再启动 Agent")?;

    let agent = Agent::new(api_key)?;
    let mode = std::env::args().nth(1);

    if mode.as_deref() == Some("tui") {
        ui::tui::run(agent).await
    } else {
        ui::cli::run(agent).await
    }
}
