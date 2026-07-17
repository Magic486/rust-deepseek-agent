---
name: code-review
description: 审查 Rust 代码中的缺陷、回归风险、安全边界和测试缺口
---

# Code Review

先读取相关实现和测试，再给出结论。发现优先于总结，并按严重程度排序。

- 每个问题指出文件位置、触发条件和实际影响。
- 优先检查错误处理、路径边界、异步阻塞、状态一致性和敏感信息泄漏。
- 检查修改是否破坏 CLI、TUI、Tool、Memory、Todo、Skill、MCP 或 Sub-Agent 的共享契约。
- 没有发现问题时明确说明，并指出仍未覆盖的测试风险。
