//! `web` tool — fetch URL content (Phase M9).

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::providers::ToolSpec;

use super::{Tool, ToolContext, ToolResult};

const MAX_BYTES: usize = 512_000;

pub struct WebTool;

#[async_trait]
impl Tool for WebTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web".to_owned(),
            description: "Fetch a public HTTP(S) URL and return text content (truncated)."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: ToolContext) -> ToolResult {
        let Some(url) = args.get("url").and_then(|v| v.as_str()) else {
            return ToolResult::error(ctx.call_id, "missing required parameter: url".to_owned());
        };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return ToolResult::error(ctx.call_id, "url must be http:// or https://".to_owned());
        }

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("artui/0.7")
            .build()
        {
            Ok(c) => c,
            Err(e) => return ToolResult::error(ctx.call_id, format!("http client: {e}")),
        };

        let resp = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::error(ctx.call_id, format!("fetch failed: {e}")),
        };
        let status = resp.status();
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => return ToolResult::error(ctx.call_id, format!("read body: {e}")),
        };
        let mut body = String::from_utf8_lossy(&bytes).into_owned();
        if body.len() > MAX_BYTES {
            body.truncate(MAX_BYTES);
            body.push_str("\n... (truncated)");
        }
        ToolResult::ok(
            ctx.call_id,
            format!("status: {status}\nurl: {url}\n\n{body}"),
        )
    }
}
