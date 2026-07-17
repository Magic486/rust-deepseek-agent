use std::io::{self, Write};

use anyhow::Result;

use crate::agent::{Agent, AgentEvent, AgentStatus};

pub async fn run(mut agent: Agent) -> Result<()> {
    println!(
        "DeepSeek Rust Agent 已启动。输入 /tools 查看工具，输入 /help 查看系统命令，输入 exit 退出。\n"
    );

    loop {
        print!("你：");
        io::stdout().flush()?;

        let mut user_input = String::new();
        io::stdin().read_line(&mut user_input)?;
        let result = agent.handle_user_input(user_input.trim()).await?;
        print_events(&result.events);

        if result.should_exit {
            break;
        }
    }

    Ok(())
}

pub fn print_events(events: &[AgentEvent]) {
    for event in events {
        match event {
            AgentEvent::UserMessage(_) | AgentEvent::TodoUpdated => {}
            AgentEvent::AssistantMessage(answer) => println!("\nDeepSeek：{answer}\n"),
            AgentEvent::ToolCall { name, input } => {
                println!("\nAI 决定调用工具：{name}，输入：{input}\n");
            }
            AgentEvent::ToolResult { name, output } => {
                println!("工具 {name}：\n{output}\n");
            }
            AgentEvent::ToolError { name, error } => {
                println!("工具 {name} 出错：{error}\n");
            }
            AgentEvent::SystemMessage(message) => println!("\n{message}\n"),
            AgentEvent::StatusChanged(status) => match status {
                AgentStatus::RunningTool(name) => println!("正在执行工具：{name}"),
                AgentStatus::Error(error) => println!("状态错误：{error}"),
                AgentStatus::Ready | AgentStatus::Thinking => {}
            },
        }
    }
}
