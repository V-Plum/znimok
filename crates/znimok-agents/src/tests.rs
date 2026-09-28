//! The whole path: MCP messages in → tools → permissions → library files and journal.

use crate::audit::Audit;
use crate::backend::{Capturer, Gui, Shot};
use crate::library::Library;
use crate::mcp::{MODERN, Server};
use crate::permissions::{Grant, Permissions, Scope};
use crate::tools::Agent;
use serde_json::{Value, json};
use std::path::PathBuf;
use znimok_core::{Raster, Rgb};
use znimok_platform::{CaptureTarget, DisplayId, DisplayInfo, Rect, WindowInfo};

/// A screen of 120×80 grey pixels, one display, one window.
struct FakeScreen;

impl Capturer for FakeScreen {
    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        let r = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 80,
        };
        Ok(vec![DisplayInfo {
            id: DisplayId("d1".into()),
            name: "Test".into(),
            bounds: r,
            work_area: r,
            scale_factor: 1.0,
            pixels_per_unit: 1.0,
            primary: true,
            refresh_hz: None,
            color: znimok_platform::ColorInfo::SDR,
        }])
    }
    fn windows(&self) -> Result<Vec<WindowInfo>, String> {
        Ok(vec![])
    }
    fn take(&self, t: &CaptureTarget) -> Result<Shot, String> {
        let (w, h) = match t {
            CaptureTarget::Region { rect } => (rect.width, rect.height),
            _ => (120, 80),
        };
        Ok(Shot::Pixels(Raster::solid(w, h, Rgb::new(200, 200, 200))))
    }
}

struct Env {
    agent: Agent,
    dir: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn env(tag: &str, enabled: bool) -> Env {
    let dir = std::env::temp_dir().join(format!("znimok-agents-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // An IPC config nobody listens on: «no app running».
    let gui = Gui::with_config(znimok_ipc::Config {
        suffix: Some(format!("n{tag}{}", std::process::id())),
        dir: Some(dir.join("ipc")),
        ..Default::default()
    });
    Env {
        agent: Agent {
            lib: Library {
                dir: dir.join("lib"),
            },
            perms: Permissions::new(dir.join("agents.json")),
            audit: Some(Audit::open(dir.join("audit.jsonl"))),
            gui,
            capture: Box::new(FakeScreen),
            enabled,
            export_dir: dir.join("exports"),
        },
        dir,
    }
}

fn req(id: u64, method: &str, params: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

fn modern(mut params: Value) -> Value {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": {"name": "Claude Code", "version": "9"},
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    params
}

#[test]
fn legacy_handshake_then_capture_and_annotate() {
    let e = env("legacy", true);
    e.agent
        .perms
        .grant("Claude Desktop", Scope::Capture, Grant::Always)
        .unwrap();
    e.agent
        .perms
        .grant("Claude Desktop", Scope::LibraryWrite, Grant::Always)
        .unwrap();
    let mut s = Server::new(&e.agent);
    let init = s
        .handle_line(&req(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "Claude Desktop", "version": "1"}
            }),
        ))
        .unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(
        s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none()
    );

    let list = s.handle_line(&req(2, "tools/list", json!({}))).unwrap();
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names[..3],
        ["list_displays", "list_windows", "capture_screen"]
    );
    assert!(
        list["result"].get("resultType").is_none(),
        "legacy results stay plain"
    );

    let shot = s
        .handle_line(&req(
            3,
            "tools/call",
            json!({"name": "capture_region",
            "arguments": {"x": 10, "y": 10, "width": 40, "height": 30}}),
        ))
        .unwrap();
    let r = &shot["result"];
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["structuredContent"]["width"], 40);
    let kinds: Vec<&str> = r["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["text", "image", "resource_link"]);
    let id = r["structuredContent"]["id"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(r["structuredContent"]["path"].as_str().unwrap()).exists());

    let ann = s
        .handle_line(&req(4, "tools/call", json!({"name": "annotate", "arguments": {
            "document": id,
            "commands": [{"cmd": "add_object", "object": {"rect": {"x": 2, "y": 2, "w": 20, "h": 10}, "data": {"kind": "rect"}}}]
        }})))
        .unwrap();
    assert_eq!(ann["result"]["isError"], false, "{}", ann["result"]);
    assert_eq!(ann["result"]["structuredContent"]["marks"], 1);

    let journal = e.agent.audit.as_ref().unwrap().entries();
    assert_eq!(journal.len(), 2);
    assert!(journal[0].capture && journal[0].ok);
    assert_eq!(journal[0].grant, Some(Grant::Always));
}

#[test]
fn modern_stateless_requests() {
    let e = env("modern", true);
    let mut s = Server::new(&e.agent);
    let d = s
        .handle_line(&req(1, "server/discover", modern(json!({}))))
        .unwrap();
    assert_eq!(d["result"]["supportedVersions"][0], MODERN);
    assert_eq!(d["result"]["resultType"], "complete");
    assert_eq!(
        d["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "znimok"
    );

    let l = s
        .handle_line(&req(2, "tools/list", modern(json!({}))))
        .unwrap();
    assert_eq!(l["result"]["resultType"], "complete");
    assert_eq!(l["result"]["cacheScope"], "private");
    assert!(l["result"]["ttlMs"].as_u64().unwrap() > 0);

    let mut old = modern(json!({}));
    old["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("1900-01-01");
    let bad = s.handle_line(&req(3, "tools/list", old)).unwrap();
    assert_eq!(bad["error"]["code"], -32022);
    assert_eq!(bad["error"]["data"]["requested"], "1900-01-01");
    assert!(
        bad["error"]["data"]["supported"]
            .as_array()
            .unwrap()
            .contains(&json!("2025-11-25"))
    );

    // No permission and no app to ask: refused with instructions, journal says so.
    let c = s
        .handle_line(&req(
            4,
            "tools/call",
            modern(json!({"name": "capture_screen", "arguments": {}})),
        ))
        .unwrap();
    assert_eq!(c["result"]["isError"], true);
    let text = c["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("znimok agents allow \"Claude Code\" capture"),
        "{text}"
    );
    let j = e.agent.audit.as_ref().unwrap().entries();
    assert_eq!(j.len(), 1);
    assert!(!j[0].ok && !j[0].capture && j[0].grant.is_none());

    // list_displays needs no permission.
    let ds = s
        .handle_line(&req(
            5,
            "tools/call",
            modern(json!({"name": "list_displays", "arguments": {}})),
        ))
        .unwrap();
    assert_eq!(ds["result"]["structuredContent"]["displays"][0]["id"], "d1");

    let unknown = s
        .handle_line(&req(6, "prompts/list", modern(json!({}))))
        .unwrap();
    assert_eq!(unknown["error"]["code"], -32601);
}

#[test]
fn switched_off_refuses_every_tool() {
    let e = env("off", false);
    let mut s = Server::new(&e.agent);
    let c = s
        .handle_line(&req(
            1,
            "tools/call",
            modern(json!({"name": "list_displays", "arguments": {}})),
        ))
        .unwrap();
    assert_eq!(c["result"]["isError"], true);
    assert!(
        c["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("switched off")
    );
}

#[test]
fn library_resources_search_export_and_redaction() {
    let e = env("e2e", true);
    for s in [Scope::Capture, Scope::LibraryRead, Scope::LibraryWrite] {
        e.agent
            .perms
            .grant("Claude Code", s, Grant::Always)
            .unwrap();
    }
    let mut s = Server::new(&e.agent);
    let shot = s
        .handle_line(&req(
            1,
            "tools/call",
            modern(json!({"name": "capture_screen", "arguments": {}})),
        ))
        .unwrap();
    let id = shot["result"]["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let found = s
        .handle_line(&req(
            2,
            "tools/call",
            modern(json!({"name": "library_search", "arguments": {"query": ""}})),
        ))
        .unwrap();
    assert_eq!(
        found["result"]["structuredContent"]["documents"][0]["id"],
        id.as_str()
    );

    let res = s
        .handle_line(&req(3, "resources/list", modern(json!({}))))
        .unwrap();
    let uri = res["result"]["resources"][0]["uri"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(uri, format!("znimok://library/{id}"));
    let read = s
        .handle_line(&req(4, "resources/read", modern(json!({"uri": uri}))))
        .unwrap();
    assert_eq!(read["result"]["contents"][0]["mimeType"], "image/png");
    let missing = s
        .handle_line(&req(
            5,
            "resources/read",
            modern(json!({"uri": "znimok://library/nope"})),
        ))
        .unwrap();
    assert_eq!(missing["error"]["code"], -32602);

    let out = s
        .handle_line(&req(
            6,
            "tools/call",
            modern(json!({"name": "export", "arguments": {"document": id, "format": "html"}})),
        ))
        .unwrap();
    let path = out["result"]["structuredContent"]["path"]
        .as_str()
        .unwrap_or_else(|| panic!("{}", out["result"]));
    assert!(
        path.ends_with(".html") && std::path::Path::new(path).exists(),
        "{path}"
    );

    let red = s
        .handle_line(&req(7, "tools/call", modern(json!({"name": "redact_pii", "arguments": {"document": id, "apply": false, "faces": false}}))))
        .unwrap();
    assert_eq!(red["result"]["isError"], false, "{}", red["result"]);
    assert_eq!(red["result"]["structuredContent"]["applied"], false);
}

#[test]
fn bad_input_is_answered_not_fatal() {
    let e = env("bad", true);
    let mut s = Server::new(&e.agent);
    assert_eq!(s.handle_line("{nope").unwrap()["error"]["code"], -32700);
    assert_eq!(s.handle_line("[]").unwrap()["error"]["code"], -32600);
    let c = s
        .handle_line(&req(
            1,
            "tools/call",
            modern(json!({"name": "no_such_tool", "arguments": {}})),
        ))
        .unwrap();
    assert_eq!(c["result"]["isError"], true);
}

/// docs/AGENTS.md describes every tool, and its command examples are valid for the editor.
#[test]
fn agents_doc_matches_the_tools() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc = std::fs::read_to_string(root.join("docs/AGENTS.md")).unwrap();
    for t in crate::tools::list() {
        let name = t["name"].as_str().unwrap();
        assert!(
            doc.contains(&format!("`{name}`")),
            "docs/AGENTS.md does not describe {name}"
        );
    }
    // Every JSON line that starts a command in the examples block.
    let block = doc
        .split("### Annotate: commands")
        .nth(1)
        .unwrap()
        .split("## Scenarios")
        .next()
        .unwrap();
    let json_block = block
        .split("```json")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let mut buf = String::new();
    let mut n = 0;
    let mut ed = znimok_core::Editor::new(znimok_core::Document::from_raster(
        "t",
        Raster::solid(900, 700, Rgb::WHITE),
    ));
    for line in json_block.lines() {
        buf.push_str(line);
        if let Ok(v) = serde_json::from_str::<Value>(&buf) {
            let cmd: znimok_core::Command =
                serde_json::from_value(v).unwrap_or_else(|e| panic!("{buf}: {e}"));
            ed.apply(cmd).unwrap_or_else(|e| panic!("{buf}: {e}"));
            buf.clear();
            n += 1;
        }
    }
    assert_eq!(n, 4, "all four examples parsed");
    assert!(buf.trim().is_empty(), "left over: {buf}");
    let skill = std::fs::read_to_string(root.join("packaging/skill/znimok/SKILL.md")).unwrap();
    for t in crate::tools::list() {
        assert!(
            skill.contains(t["name"].as_str().unwrap()),
            "SKILL.md misses {}",
            t["name"]
        );
    }
}
