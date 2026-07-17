use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

#[derive(Clone)]
pub struct TodoItem {
    pub id: usize,
    pub title: String,
    pub status: TodoStatus,
}

#[derive(Clone)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
    Blocked,
}

#[derive(Deserialize)]
pub struct TodoUpdateRequest {
    pub todos: Vec<TodoUpdateItem>,
}

#[derive(Deserialize)]
pub struct TodoUpdateItem {
    pub id: usize,
    pub title: String,
    pub status: String,
}

pub struct TodoList {
    items: Vec<TodoItem>,
}

impl TodoList {
    pub fn new_session() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, title: &str) -> Result<TodoItem> {
        if title.trim().is_empty() {
            return Err(anyhow!("待办内容不能为空"));
        }

        let item = TodoItem {
            id: self.next_id(),
            title: title.trim().to_string(),
            status: TodoStatus::Pending,
        };

        self.items.push(item.clone());
        Ok(item)
    }

    pub fn done(&mut self, id: usize) -> Result<String> {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return Err(anyhow!("没有找到待办 #{id}"));
        };

        item.status = TodoStatus::Done;
        let title = item.title.clone();
        Ok(format!("已完成待办 #{id}：{title}"))
    }

    pub fn update_all_from_json(&mut self, input: &str) -> Result<String> {
        let request: TodoUpdateRequest = serde_json::from_str(input).context(
            "todo_update 输入必须是 JSON，例如 {\"todos\":[{\"id\":1,\"title\":\"任务\",\"status\":\"in_progress\"}]}",
        )?;
        self.update_all(request.todos)
    }

    pub fn update_all(&mut self, updates: Vec<TodoUpdateItem>) -> Result<String> {
        let mut items = Vec::new();

        for update in updates {
            let title = update.title.trim();
            if title.is_empty() {
                return Err(anyhow!("待办标题不能为空"));
            }

            items.push(TodoItem {
                id: update.id,
                title: title.to_string(),
                status: parse_status(&update.status)?,
            });
        }

        let in_progress_count = items
            .iter()
            .filter(|item| matches!(item.status, TodoStatus::InProgress))
            .count();
        if in_progress_count > 1 {
            return Err(anyhow!("同一时间只能有一个 in_progress 待办"));
        }

        items.sort_by_key(|item| item.id);
        self.items = items;

        Ok(format!("Todo 已更新：\n{}", self.list()))
    }

    pub fn list(&self) -> String {
        if self.items.is_empty() {
            return "还没有待办。".to_string();
        }

        self.items
            .iter()
            .map(|item| {
                let mark = match item.status {
                    TodoStatus::Pending => "[ ]",
                    TodoStatus::InProgress => "[~]",
                    TodoStatus::Done => "[x]",
                    TodoStatus::Blocked => "[!]",
                };
                format!("{mark} #{} {}", item.id, item.title)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn items(&self) -> &[TodoItem] {
        &self.items
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    fn next_id(&self) -> usize {
        self.items.iter().map(|item| item.id).max().unwrap_or(0) + 1
    }
}

pub fn status_label(status: &TodoStatus) -> &'static str {
    match status {
        TodoStatus::Pending => "Pending",
        TodoStatus::InProgress => "InProgress",
        TodoStatus::Done => "Done",
        TodoStatus::Blocked => "Blocked",
    }
}

fn parse_status(status: &str) -> Result<TodoStatus> {
    match status.trim().to_ascii_lowercase().as_str() {
        "pending" => Ok(TodoStatus::Pending),
        "in_progress" | "inprogress" => Ok(TodoStatus::InProgress),
        "done" | "completed" => Ok(TodoStatus::Done),
        "blocked" => Ok(TodoStatus::Blocked),
        other => Err(anyhow!(
            "未知 Todo 状态：{other}，可用状态是 pending/in_progress/done/blocked"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::TodoList;

    #[test]
    fn new_session_does_not_restore_previous_todos() {
        let mut first_session = TodoList::new_session();
        first_session.add("完成当前任务").unwrap();
        assert_eq!(first_session.items().len(), 1);

        let second_session = TodoList::new_session();
        assert!(second_session.items().is_empty());
    }

    #[test]
    fn clear_removes_all_session_todos() {
        let mut todos = TodoList::new_session();
        todos.add("任务一").unwrap();
        todos.add("任务二").unwrap();

        todos.clear();

        assert!(todos.items().is_empty());
    }

    #[test]
    fn update_all_allows_one_in_progress() {
        let mut todos = TodoList::new_session();
        todos
            .update_all_from_json(
                r#"{"todos":[{"id":1,"title":"读文件","status":"in_progress"},{"id":2,"title":"写总结","status":"pending"}]}"#,
            )
            .unwrap();

        assert_eq!(todos.items().len(), 2);
    }

    #[test]
    fn update_all_rejects_two_in_progress() {
        let mut todos = TodoList::new_session();
        let error = todos
            .update_all_from_json(
                r#"{"todos":[{"id":1,"title":"读文件","status":"in_progress"},{"id":2,"title":"写总结","status":"in_progress"}]}"#,
            )
            .unwrap_err();

        assert!(error.to_string().contains("只能有一个"));
    }
}
