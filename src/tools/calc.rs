use anyhow::{Context, Result};

pub fn run(input: &str) -> Result<String> {
    let parts: Vec<&str> = input.split_whitespace().collect();

    if parts.len() != 3 {
        return Ok("格式是：/calc 数字 运算符 数字，比如 /calc 12 * 8".to_string());
    }

    let left: f64 = parts[0].parse().context("左边不是有效数字")?;
    let operator = parts[1];
    let right: f64 = parts[2].parse().context("右边不是有效数字")?;

    let result = match operator {
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" => {
            if right == 0.0 {
                return Ok("除数不能是 0".to_string());
            }
            left / right
        }
        _ => return Ok("只支持 +、-、*、/ 四种运算符".to_string()),
    };

    Ok(result.to_string())
}
