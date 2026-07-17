use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde::Deserialize;

use crate::config;

const BRAVE_SEARCH_API_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const MAX_WEB_TEXT_CHARS: usize = 12_000;

#[derive(Deserialize)]
struct BraveSearchResponse {
    web: Option<BraveWeb>,
}

#[derive(Deserialize)]
struct BraveWeb {
    results: Vec<BraveSearchResult>,
}

#[derive(Deserialize)]
struct BraveSearchResult {
    title: String,
    url: String,
    description: Option<String>,
}

pub async fn search(client: &Client, input: &str) -> Result<String> {
    if input.trim().is_empty() {
        return Ok("格式是：/web_search 搜索关键词".to_string());
    }

    match config::env_var("BRAVE_SEARCH_API_KEY") {
        Ok(api_key) => match brave_search(client, input, &api_key).await {
            Ok(output) => Ok(output),
            Err(error) => {
                let fallback = duckduckgo_lite_search(client, input).await?;
                Ok(format!(
                    "Brave Search 调用失败，已自动改用 DuckDuckGo Lite。\n错误：{error}\n\n{fallback}"
                ))
            }
        },
        Err(_) => duckduckgo_lite_search(client, input).await,
    }
}

async fn brave_search(client: &Client, input: &str, api_key: &str) -> Result<String> {
    let response = client
        .get(BRAVE_SEARCH_API_URL)
        .header("X-Subscription-Token", api_key)
        .header("Accept", "application/json")
        .query(&[("q", input), ("count", "5")])
        .send()
        .await
        .context("请求 Web Search API 失败")?
        .error_for_status()
        .context("Web Search API 返回了错误状态码")?
        .json::<BraveSearchResponse>()
        .await
        .context("解析 Web Search API 响应失败")?;

    let Some(web) = response.web else {
        return Ok("没有搜索到网页结果".to_string());
    };

    let results: Vec<String> = web
        .results
        .into_iter()
        .take(5)
        .enumerate()
        .map(|(index, result)| {
            let description = result.description.unwrap_or_default();
            format!(
                "{}. {}\n   URL: {}\n   摘要: {}",
                index + 1,
                result.title,
                result.url,
                description
            )
        })
        .collect();

    if results.is_empty() {
        Ok("没有搜索到网页结果".to_string())
    } else {
        Ok(format!(
            "搜索提供方：Brave Search\n\n{}",
            results.join("\n\n")
        ))
    }
}

async fn duckduckgo_lite_search(client: &Client, input: &str) -> Result<String> {
    let query = encode_query(input);
    let url = format!("https://lite.duckduckgo.com/lite/?q={query}&kl=us-en");
    let response = client
        .get(&url)
        .header(
            "User-Agent",
            "rust-deepseek-agent/0.1 (+https://local.agent)",
        )
        .send()
        .await
        .context("请求 DuckDuckGo Lite 失败")?
        .error_for_status()
        .context("DuckDuckGo Lite 返回了错误状态码")?;

    let body = response
        .text()
        .await
        .context("读取 DuckDuckGo Lite 内容失败")?;
    let text = collapse_blank_lines(&html_to_text(&body));
    let text = truncate_to_chars(&text, 8_000);

    Ok(format!(
        "搜索提供方：DuckDuckGo Lite（未配置 BRAVE_SEARCH_API_KEY 时的免费兜底）\n搜索 URL: {url}\n\n{text}"
    ))
}

pub async fn fetch(client: &Client, input: &str) -> Result<String> {
    let url = input.trim();

    if url.is_empty() {
        return Ok("格式是：/web_fetch https://example.com/page".to_string());
    }

    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(anyhow!("web_fetch 只支持 http:// 或 https:// URL"));
    }

    let response = client
        .get(url)
        .header(
            "User-Agent",
            "rust-deepseek-agent/0.1 (+https://local.agent)",
        )
        .send()
        .await
        .context("请求网页失败")?
        .error_for_status()
        .context("网页返回了错误状态码")?;

    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.context("读取网页内容失败")?;

    let text = if content_type.contains("text/html") || looks_like_html(&body) {
        html_to_text(&body)
    } else {
        body
    };

    let text = collapse_blank_lines(&text);
    let text = truncate_text(&text);

    Ok(format!(
        "URL: {final_url}\nContent-Type: {content_type}\n\n{text}"
    ))
}

fn looks_like_html(input: &str) -> bool {
    let lower = input
        .chars()
        .take(500)
        .collect::<String>()
        .to_ascii_lowercase();
    lower.contains("<html") || lower.contains("<body") || lower.contains("<!doctype html")
}

fn html_to_text(input: &str) -> String {
    let without_scripts = remove_tag_blocks(input, "script");
    let without_styles = remove_tag_blocks(&without_scripts, "style");
    let mut output = String::new();
    let mut in_tag = false;

    for character in without_styles.chars() {
        match character {
            '<' => {
                in_tag = true;
                output.push('\n');
            }
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }

    decode_basic_entities(&output)
}

fn remove_tag_blocks(input: &str, tag: &str) -> String {
    let mut rest = input.to_string();
    let open_tag = format!("<{tag}");
    let close_tag = format!("</{tag}>");

    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find(&open_tag) else {
            break;
        };
        let Some(relative_end) = lower[start..].find(&close_tag) else {
            break;
        };
        let end = start + relative_end + close_tag.len();
        rest.replace_range(start..end, "");
    }

    rest
}

fn decode_basic_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn collapse_blank_lines(input: &str) -> String {
    let mut lines = Vec::new();
    let mut previous_blank = false;

    for line in input.lines() {
        let trimmed = line.trim();
        let is_blank = trimmed.is_empty();

        if is_blank {
            if !previous_blank {
                lines.push(String::new());
            }
        } else {
            lines.push(trimmed.to_string());
        }

        previous_blank = is_blank;
    }

    lines.join("\n").trim().to_string()
}

fn truncate_text(input: &str) -> String {
    if input.chars().count() <= MAX_WEB_TEXT_CHARS {
        return input.to_string();
    }

    let preview: String = input.chars().take(MAX_WEB_TEXT_CHARS).collect();
    format!("{preview}\n\n[网页内容较长，只显示前 {MAX_WEB_TEXT_CHARS} 个字符]")
}

fn truncate_to_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }

    let preview: String = input.chars().take(max_chars).collect();
    format!("{preview}\n\n[搜索结果较长，只显示前 {max_chars} 个字符]")
}

fn encode_query(input: &str) -> String {
    let mut output = String::new();

    for byte in input.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char);
            }
            b' ' => output.push('+'),
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }

    output
}
