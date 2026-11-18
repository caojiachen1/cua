//! MCP stdio adapter: exposes the browser tool surface to any MCP client
//! (Claude Desktop, IDE agents, ...) as newline-delimited JSON-RPC 2.0 on
//! stdin/stdout. Run it with `cua browser mcp` — the IDE spawns `cua.exe`,
//! this module answers on stdio and talks to the daemon in-process, so the
//! MCP world and the CLI world share one session state.
//!
//! The tool catalog is fetched LIVE from the extension (`tool.list`) — there
//! is no second, hand-written tool list to drift out of date. Tool names map
//! to MCP's `[a-zA-Z0-9_-]` alphabet by replacing `.` with `__`
//! (`tab.create` → `tab__create`); `tools/call` accepts either spelling.
//! Binary outputs (`data_base64` from screenshots/PDF) come back as MCP image
//! content instead of megabytes of base64 text.

use crate::Launch;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write as _};

pub fn serve_stdio(launch: Launch) -> ! {
    eprintln!("[cua-browser-mcp] stdio server started");
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut names: HashMap<String, String> = HashMap::new();
    let mut timeouts: HashMap<String, u64> = HashMap::new();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[cua-browser-mcp] dropped unparseable line: {e}");
                continue;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        if id.is_null() {
            continue; // notification — nothing to answer
        }
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let reply = match dispatch(&launch, &method, &params, &mut names, &mut timeouts) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": code, "message": message, "data": Value::Null},
            }),
        };
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
    eprintln!("[cua-browser-mcp] stdin closed; exiting");
    std::process::exit(0);
}

type RpcError = (i64, String);

fn dispatch(
    launch: &Launch,
    method: &str,
    params: &Value,
    names: &mut HashMap<String, String>,
    timeouts: &mut HashMap<String, u64>,
) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2024-11-05")),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "cua-browser", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => tools_list(launch, names, timeouts),
        "tools/call" => tools_call(launch, params, names, timeouts),
        _ => Err((-32601, format!("method \"{method}\" not found"))),
    }
}

fn connect(launch: &Launch) -> Result<(u16, String), RpcError> {
    let (token, port) = crate::config::token_and_port().map_err(|e| (-32000, e))?;
    let port = std::env::var("CUA_BROWSER_PORT")
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .unwrap_or(port);
    crate::client::ensure(port, launch).map_err(|e| (-32000, format!("{e}")))?;
    Ok((port, token))
}

fn tools_list(
    launch: &Launch,
    names: &mut HashMap<String, String>,
    timeouts: &mut HashMap<String, u64>,
) -> Result<Value, RpcError> {
    let (port, token) = connect(launch)?;
    let r = crate::client::call(port, &token, "tool.list", &Value::Null, None, 30_000)
        .map_err(|e| (-32000, format!("{e}")))?;
    let catalog = r.get("result").cloned().unwrap_or(json!({}));
    let tools = catalog
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<Value> = Vec::with_capacity(tools.len());
    names.clear();
    timeouts.clear();
    for t in &tools {
        let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let mcp_name = name.replace('.', "__");
        names.insert(mcp_name.clone(), name.clone());
        let timeout = t.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(120_000);
        timeouts.insert(mcp_name.clone(), timeout);
        let category = t.get("category").and_then(|v| v.as_str()).unwrap_or("general");
        let description = t.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let sensitive = t.get("sensitive").and_then(|v| v.as_bool()).unwrap_or(false);
        let desc = if sensitive {
            format!("[{category}] {description} (sensitive: needs confirm:true)")
        } else {
            format!("[{category}] {description}")
        };
        out.push(json!({
            "name": mcp_name,
            "description": desc,
            "inputSchema": { "type": "object", "additionalProperties": true },
        }));
    }
    Ok(json!({ "tools": out }))
}

/// MCP tool-execution failures are reported as `isError` content, not as
/// JSON-RPC errors — the protocol error channel is for protocol failures.
fn tools_call(
    launch: &Launch,
    params: &Value,
    names: &HashMap<String, String>,
    timeouts: &HashMap<String, u64>,
) -> Result<Value, RpcError> {
    let raw = params.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let tool = if raw.contains('.') {
        raw.clone()
    } else if let Some(real) = names.get(&raw) {
        real.clone()
    } else {
        raw.replace("__", ".")
    };
    if tool.is_empty() {
        return Err((-32602, "tools/call needs {name, arguments}".to_string()));
    }
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let (port, token) = connect(launch)?;
    let timeout_ms = timeouts.get(&raw).copied().unwrap_or(120_000).min(900_000) + 5_000;
    let res = crate::client::call(port, &token, &tool, &args, None, timeout_ms);
    let (payload, binary) = match res {
        Ok(msg) => {
            let mut result = msg.get("result").cloned().unwrap_or(json!({}));
            let binary = result.get("data_base64").and_then(|v| v.as_str()).map(String::from);
            if let Some(o) = result.as_object_mut() {
                o.remove("data_base64");
            }
            (result, binary)
        }
        Err(crate::client::CallError::Failed(e)) => {
            let msg = match (e.get("code").and_then(|c| c.as_str()), e.get("message").and_then(|m| m.as_str())) {
                (Some(c), Some(m)) => format!("{c}: {m}"),
                _ => format!("{e}"),
            };
            return Ok(json!({
                "content": [{ "type": "text", "text": msg }],
                "isError": true,
            }));
        }
        Err(e) => return Err((-32000, format!("{e}"))),
    };
    let mut content: Vec<Value> = Vec::new();
    if let Some(b64) = &binary {
        // screenshot/element-shot output is a PNG; a PDF stays text-described
        let mime = if tool.contains("pdf") { "application/pdf" } else { "image/png" };
        if mime == "image/png" {
            content.push(json!({ "type": "image", "data": b64, "mimeType": "image/png" }));
        }
        content.push(json!({
            "type": "text",
            "text": format!("binary output {} KB ({mime}); pass it through as-is, do not read the base64 into context", b64.len() * 3 / 4 / 1024),
        }));
    }
    if let Some(text) = payload.get("text").and_then(|v| v.as_str()) {
        content.push(json!({ "type": "text", "text": text }));
    }
    let json_text = serde_json::to_string_pretty(&payload).unwrap_or_default();
    if json_text.chars().count() <= 4000 || payload.get("text").is_none() {
        content.push(json!({ "type": "text", "text": json_text }));
    } else {
        content.push(json!({
            "type": "text",
            "text": format!("(structured result too large to inline — {} chars; use narrower tools)", json_text.chars().count()),
        }));
    }
    Ok(json!({ "content": content, "isError": false }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_map_to_mcp_alphabet() {
        assert_eq!("tab.create".replace('.', "__"), "tab__create");
        assert_eq!("read_page".replace('.', "__"), "read_page");
        assert_eq!("tab__create".replace("__", "."), "tab.create");
    }

    #[test]
    fn unknown_method_is_a_protocol_error() {
        let mut names = HashMap::new();
        let mut timeouts = HashMap::new();
        let l = Launch { exe: "cua.exe".into(), subcommand: vec![] };
        let e = dispatch(&l, "resources/list", &json!({}), &mut names, &mut timeouts).unwrap_err();
        assert_eq!(e.0, -32601);
    }

    #[test]
    fn tools_call_without_name_is_invalid_params() {
        let mut names = HashMap::new();
        let mut timeouts = HashMap::new();
        let l = Launch { exe: "cua.exe".into(), subcommand: vec![] };
        let e = dispatch(&l, "tools/call", &json!({}), &mut names, &mut timeouts).unwrap_err();
        assert_eq!(e.0, -32602);
    }
}
