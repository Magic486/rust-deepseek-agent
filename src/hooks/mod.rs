use crate::memory::MemoryStore;
use crate::message::Message;

pub fn before_llm_user_message(memory: &MemoryStore, user_input: &str) -> String {
    memory.enrich_user_input(user_input)
}

pub fn after_tool_result(
    messages: &mut Vec<Message>,
    tool_name: &str,
    tool_input: &str,
    tool_output: &str,
) {
    messages.push(Message::new(
        "user",
        format!("我调用了工具 {tool_name}，输入是：{tool_input}"),
    ));

    messages.push(Message::new(
        "assistant",
        format!("工具 {tool_name} 的结果是：{tool_output}"),
    ));
}

pub fn after_agent_answer(messages: &mut Vec<Message>, answer: String) {
    messages.push(Message::new("assistant", answer));
}
