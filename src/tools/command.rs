use std::process::Command;

use anyhow::{Context, Result, anyhow};

const MAX_OUTPUT_CHARS: usize = 12_000;

pub fn run(input: &str) -> Result<String> {
    let args = split_command(input)?;

    if args.is_empty() {
        return Ok("格式是：/run_command cargo check".to_string());
    }

    ensure_allowed_command(&args)?;

    let output = Command::new(&args[0])
        .args(&args[1..])
        .output()
        .with_context(|| format!("执行命令失败：{}", args.join(" ")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut result = format!("退出码：{}\n", output.status);

    if !stdout.trim().is_empty() {
        result.push_str("\n[stdout]\n");
        result.push_str(stdout.trim_end());
        result.push('\n');
    }

    if !stderr.trim().is_empty() {
        result.push_str("\n[stderr]\n");
        result.push_str(stderr.trim_end());
        result.push('\n');
    }

    Ok(truncate_output(result))
}

pub fn validate_project() -> Result<String> {
    let checks = [
        vec!["cargo", "fmt"],
        vec!["cargo", "check"],
        vec!["cargo", "test"],
    ];
    let mut output = String::new();

    for check in checks {
        let result = run_allowed_process(&check)?;
        output.push_str(&format!("$ {}\n", check.join(" ")));
        output.push_str(&result);
        output.push('\n');

        if !result.starts_with("退出码：exit code: 0") {
            output.push_str("\n校验失败，已停止后续检查。\n");
            return Ok(truncate_output(output));
        }
    }

    output.push_str("\n项目校验完成：cargo fmt、cargo check、cargo test 均通过。");
    Ok(truncate_output(output))
}

fn ensure_allowed_command(args: &[String]) -> Result<()> {
    let command = args[0].as_str();
    let rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();

    let allowed = matches!(
        (command, rest.as_slice()),
        ("cargo", ["check"])
            | ("cargo", ["fmt"])
            | ("cargo", ["test"])
            | ("cargo", ["build"])
            | ("cargo", ["clippy"])
            | ("cargo", ["--version"])
            | ("rustc", ["--version"])
    );

    if allowed {
        Ok(())
    } else {
        Err(anyhow!(
            "run_command 只允许安全白名单命令：cargo check/fmt/test/build/clippy、cargo --version、rustc --version"
        ))
    }
}

fn run_allowed_process(args: &[&str]) -> Result<String> {
    let output = Command::new(args[0])
        .args(&args[1..])
        .output()
        .with_context(|| format!("执行命令失败：{}", args.join(" ")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut result = format!("退出码：{}\n", output.status);

    if !stdout.trim().is_empty() {
        result.push_str("\n[stdout]\n");
        result.push_str(stdout.trim_end());
        result.push('\n');
    }

    if !stderr.trim().is_empty() {
        result.push_str("\n[stderr]\n");
        result.push_str(stderr.trim_end());
        result.push('\n');
    }

    Ok(result)
}

fn split_command(input: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for character in input.trim().chars() {
        match character {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    args.push(current);
                    current = String::new();
                }
            }
            _ => current.push(character),
        }
    }

    if in_quotes {
        return Err(anyhow!("命令中的双引号没有闭合"));
    }

    if !current.is_empty() {
        args.push(current);
    }

    Ok(args)
}

fn truncate_output(output: String) -> String {
    if output.chars().count() <= MAX_OUTPUT_CHARS {
        return output;
    }

    let preview: String = output.chars().take(MAX_OUTPUT_CHARS).collect();
    format!("{preview}\n\n[输出较长，只显示前 {MAX_OUTPUT_CHARS} 个字符]")
}
