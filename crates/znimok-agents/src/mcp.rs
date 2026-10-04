//! MCP over stdio, dual-era (spec 2026-07-28 «Versioning and Compatibility»):
//!
//! - **modern** (`2026-07-28`): no handshake; every request carries
//!   `_meta["io.modelcontextprotocol/protocolVersion"]` (+ `clientInfo`, `clientCapabilities`);
//!   `server/discover` is answered; results carry `resultType` and `_meta.serverInfo`; list and
//!   read results carry `ttlMs` / `cacheScope`; an unknown version gets `-32022` with the
//!   supported list.
//! - **legacy** (`2025-11-25` and the two before it): `initialize` → `notifications/initialized`,
//!   `ping`; the negotiated version holds for the process.
//!
//! One JSON-RPC message per line on stdin, answers on stdout, diagnostics only on stderr.

use crate::tools::{self, Agent};
use serde_json::{Map, Value, json};
use std::io::{BufRead, Write};

pub const MODERN: &str = "2026-07-28";
pub const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT: &str = "io.modelcontextprotocol/clientInfo";
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";

const INSTRUCTIONS: &str = "Znimok takes screenshots and screen recordings, edits them and keeps them in a local library. \
list_targets shows the displays and windows; capture takes a screenshot of a screen, a window, the \
window in front or a region (a new library document, its id comes back). marks adds, changes and \
removes marks (list_marks gives the ids); transform crops, rotates, resizes and tones; set_meta names \
and tags; redact_pii hides secrets; ocr reads text (find looks a word up); read_codes reads codes. \
library_search finds documents (kind, tags, pinned, with a browser log); library_edit imports, copies, \
trashes and restores; library_delete deletes for good (the person confirms every time). record starts, \
pauses and stops a screen recording (sound only with the person's separate permission); video_info and \
devlog read a recording and its DevTools log, video_frames shows its frames. export writes a picture, or a recording as mp4, gif or a report; hand_over opens a document in \
the editor or puts it on the clipboard; share_targets lists the person's connected services and share \
sends a document there (its own permission, asked on its own). Reads ask nothing once allowed; the person approves a client's \
access the first time (per scope, or everything at once).";

fn server_info() -> Value {
    json!({"name": "znimok", "title": "Znimok", "version": env!("CARGO_PKG_VERSION")})
}

fn capabilities() -> Value {
    json!({
        "tools": {"listChanged": false},
        "resources": {"listChanged": false},
        "prompts": {"listChanged": false}
    })
}

/// Protocol state of one stdio process.
pub struct Server<'a> {
    agent: &'a Agent,
    /// The legacy version chosen by `initialize`, if any.
    legacy: Option<String>,
    /// The client's self-reported name (legacy: from `initialize`).
    legacy_client: String,
}

fn err(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut e = json!({"code": code, "message": message});
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": e})
}

impl<'a> Server<'a> {
    pub fn new(agent: &'a Agent) -> Self {
        Self {
            agent,
            legacy: None,
            legacy_client: "unknown client".into(),
        }
    }

    /// Runs until stdin closes.
    pub fn run(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = self.handle_line(&line) {
                let mut s = serde_json::to_string(&reply)?;
                s.push('\n');
                output.write_all(s.as_bytes())?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// One message → the answer (`None` for notifications).
    pub fn handle_line(&mut self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(err(
                    &Value::Null,
                    -32700,
                    &format!("parse error: {e}"),
                    None,
                ));
            }
        };
        if msg.is_array() {
            return Some(err(&Value::Null, -32600, "batches are not supported", None));
        }
        let method = msg.get("method").and_then(Value::as_str)?.to_string();
        let Some(id) = msg.get("id").cloned() else {
            // Notifications (notifications/initialized, cancelled…) need no answer.
            return None;
        };
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        Some(self.handle(&id, &method, &params))
    }

    fn handle(&mut self, id: &Value, method: &str, params: &Value) -> Value {
        let meta = params.get("_meta");
        let modern_version = meta
            .and_then(|m| m.get(META_VERSION))
            .and_then(Value::as_str);

        if method == "initialize" {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let v = if LEGACY.contains(&asked) {
                asked
            } else {
                LEGACY[0]
            };
            self.legacy = Some(v.to_string());
            if let Some(n) = params["clientInfo"]["name"].as_str() {
                self.legacy_client = n.to_string();
            }
            return json!({"jsonrpc": "2.0", "id": id, "result": {
                "protocolVersion": v,
                "capabilities": capabilities(),
                "serverInfo": server_info(),
                "instructions": INSTRUCTIONS,
            }});
        }

        // Modern requests declare their version; a wrong one is refused with the list.
        let modern = match modern_version {
            Some(MODERN) => true,
            Some(other) => {
                let mut supported = vec![MODERN];
                supported.extend(LEGACY);
                return err(
                    id,
                    -32022,
                    "Unsupported protocol version",
                    Some(json!({"supported": supported, "requested": other})),
                );
            }
            None => false,
        };
        if method == "server/discover" {
            let mut supported = vec![MODERN];
            supported.extend(LEGACY);
            return self.ok(
                id,
                true,
                json!({
                    "supportedVersions": supported,
                    "capabilities": capabilities(),
                    "instructions": INSTRUCTIONS,
                    "ttlMs": 3_600_000, "cacheScope": "private",
                }),
            );
        }
        if !modern && self.legacy.is_none() && method != "ping" {
            // Neither a handshake nor per-request metadata: be lenient, serve it as legacy.
            self.legacy = Some(LEGACY[0].into());
        }
        let client = if modern {
            meta.and_then(|m| m.get(META_CLIENT))
                .and_then(|c| c.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("unknown client")
                .to_string()
        } else {
            self.legacy_client.clone()
        };

        match method {
            "ping" => self.ok(id, modern, json!({})),
            "tools/list" => self.ok(
                id,
                modern,
                cacheable(json!({"tools": tools::list()}), modern),
            ),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let out = self.agent.call(&client, name, &args);
                let mut r = json!({"content": out.content, "isError": out.is_error});
                if let Some(s) = out.structured {
                    r["structuredContent"] = s;
                }
                self.ok(id, modern, r)
            }
            // Ready scenarios (ZK-236): no permission — they are text, the tools ask.
            "prompts/list" => self.ok(
                id,
                modern,
                cacheable(json!({"prompts": crate::prompts::list()}), modern),
            ),
            "prompts/get" => {
                let name = params["name"].as_str().unwrap_or("");
                match crate::prompts::get(name, params.get("arguments")) {
                    Some(p) => self.ok(id, modern, p),
                    None => err(id, -32602, "Prompt not found", Some(json!({"name": name}))),
                }
            }
            "resources/list" => match self.agent.resources(&client) {
                Ok(list) => self.ok(id, modern, cacheable(json!({"resources": list}), modern)),
                Err(e) => err(id, -32000, &e, None),
            },
            "resources/templates/list" => self.ok(
                id,
                modern,
                cacheable(json!({"resourceTemplates": []}), modern),
            ),
            "resources/read" => {
                let uri = params["uri"].as_str().unwrap_or("");
                match self.agent.read_resource(&client, uri) {
                    Ok(c) => self.ok(id, modern, cacheable(json!({"contents": [c]}), modern)),
                    Err(e) if e == "unknown resource" => {
                        err(id, -32602, "Resource not found", Some(json!({"uri": uri})))
                    }
                    Err(e) => err(id, -32000, &e, None),
                }
            }
            _ => err(id, -32601, &format!("method not found: {method}"), None),
        }
    }

    fn ok(&self, id: &Value, modern: bool, mut result: Value) -> Value {
        if modern {
            let m = result.as_object_mut().expect("object result");
            m.insert("resultType".into(), json!("complete"));
            let mut meta = Map::new();
            meta.insert(META_SERVER.into(), server_info());
            m.insert("_meta".into(), Value::Object(meta));
        }
        json!({"jsonrpc": "2.0", "id": id, "result": result})
    }
}

/// List/read results of the modern protocol say how long they may be cached; the library
/// changes whenever a screenshot is taken, so briefly and privately.
fn cacheable(mut v: Value, modern: bool) -> Value {
    if modern {
        v["ttlMs"] = json!(5_000);
        v["cacheScope"] = json!("private");
    }
    v
}
