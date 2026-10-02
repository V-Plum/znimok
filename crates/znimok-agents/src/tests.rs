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

/// The annotate input schema resolves its `$ref`s from its own root.
#[test]
fn annotate_schema_references_resolve() {
    let tools = crate::tools::list();
    let t = tools.iter().find(|t| t["name"] == "annotate").unwrap();
    let s = &t["inputSchema"];
    let text = s.to_string();
    let n = text.matches("\"$ref\":\"#/$defs/").count();
    assert!(n > 10, "{n}");
    for part in text.split("\"$ref\":\"#/$defs/").skip(1) {
        let name = part.split('"').next().unwrap();
        assert!(s["$defs"].get(name).is_some(), "{name} does not resolve");
    }
}

/// `export` never overwrites and only writes what it says it writes (ZK-113).
#[test]
fn export_does_not_clobber_files() {
    let e = env("clobber", true);
    for s in [Scope::Capture, Scope::LibraryRead] {
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
    let victim = e.dir.join("notes.txt");
    std::fs::create_dir_all(&e.dir).unwrap();
    std::fs::write(&victim, "important").unwrap();
    let call = |s: &mut Server, n, path: &PathBuf, fmt: &str| {
        s.handle_line(&req(
            n,
            "tools/call",
            modern(json!({"name": "export",
            "arguments": {"document": id, "format": fmt, "path": path.display().to_string()}})),
        ))
        .unwrap()["result"]
            .clone()
    };
    let r = call(&mut s, 2, &victim, "png");
    assert_eq!(r["isError"], true, "wrong extension refused");
    let existing = e.dir.join("old.png");
    std::fs::write(&existing, "keep").unwrap();
    let r = call(&mut s, 3, &existing, "png");
    assert_eq!(r["isError"], true, "existing file refused");
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "important");
    let fresh = e.dir.join("new.png");
    assert_eq!(call(&mut s, 4, &fresh, "png")["isError"], false);
    assert!(fresh.exists());
}

/// ZK-146: `read_codes` reads a library document with library_read only, the screen with
/// capture only, and reading the screen adds no document.
#[test]
fn read_codes_on_a_document_and_on_the_screen() {
    let e = env("codes", true);
    let call = |s: &mut Server, id: u64, args: Value| {
        s.handle_line(&req(
            id,
            "tools/call",
            modern(json!({"name": "read_codes", "arguments": args})),
        ))
        .unwrap()["result"]
            .clone()
    };
    let (w, h, rgba) = znimok_codes::qr_rgba("https://example.org/znimok", 6).unwrap();
    let doc = znimok_core::Document::from_raster("qr", Raster::new(w, h, rgba));
    let path = e.agent.lib.new_path(&doc);
    crate::library::save(&path, &doc).unwrap();
    let id = doc.id.to_string();

    e.agent
        .perms
        .grant("Claude Code", Scope::LibraryRead, Grant::Always)
        .unwrap();
    let mut s = Server::new(&e.agent);
    let r = call(&mut s, 1, json!({"document": id}));
    assert_eq!(r["isError"], false, "{r}");
    let c = &r["structuredContent"]["codes"][0];
    assert_eq!(c["kind"], "link");
    assert_eq!(c["url"], "https://example.org/znimok");
    assert_eq!(r["structuredContent"]["document"], id.as_str());

    // The screen needs capture, not granted yet (and no app to ask): refused.
    let r = call(&mut s, 2, json!({}));
    assert_eq!(r["isError"], true, "{r}");
    e.agent
        .perms
        .grant("Claude Code", Scope::Capture, Grant::Always)
        .unwrap();
    let before = e.agent.lib.search("", None, 100).len();
    let r = call(
        &mut s,
        3,
        json!({"x": 0, "y": 0, "width": 50, "height": 40}),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["structuredContent"]["codes"], json!([]));
    assert_eq!(r["structuredContent"]["screen"], true);
    assert_eq!(
        e.agent.lib.search("", None, 100).len(),
        before,
        "no new document"
    );

    // Half a region, or a document and a place on the screen, are mistakes.
    assert_eq!(call(&mut s, 4, json!({"x": 0, "y": 0}))["isError"], true);
    assert_eq!(
        call(&mut s, 5, json!({"document": id, "display": "d1"}))["isError"],
        true
    );
}

/// ZK-233: the editing tools — marks by plain arguments, then change, delete, crop, turn, resize,
/// tone; every call saves and the next one sees it.
#[test]
fn editing_tools_over_plain_arguments() {
    let e = env("edit", true);
    for s in [Scope::Capture, Scope::LibraryRead, Scope::LibraryWrite] {
        e.agent.perms.grant("T", s, Grant::Always).unwrap();
    }
    let call = |name: &str, args: Value| {
        let out = e.agent.call("T", name, &args);
        assert!(!out.is_error, "{name}: {:?}", out.content);
        out.structured.unwrap()
    };
    let shot = call(
        "capture_region",
        json!({"x": 0, "y": 0, "width": 200, "height": 120}),
    );
    let id = shot["id"].as_str().unwrap().to_string();
    let added = call(
        "add_marks",
        json!({"document": id, "marks": [
            {"kind": "rect", "x": 10, "y": 10, "width": 60, "height": 30, "color": "#00AA00", "fill": "yellow"},
            {"kind": "arrow", "from": [20, 100], "to": [120, 60]},
            {"kind": "text", "x": 80, "y": 12, "text": "Натисніть тут", "size": 18, "bold": true},
            {"kind": "counter", "x": 150, "y": 30, "shape": "pin"},
            {"kind": "hide", "x": 100, "y": 80, "width": 60, "height": 20, "mode": "plate"},
            {"kind": "highlighter", "x": 10, "y": 60, "width": 80, "height": 14},
            {"kind": "pen", "points": [[5, 5], [15, 9], [30, 4]]},
        ]}),
    );
    let created: Vec<u64> = added["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(created.len(), 7, "{added}");
    assert_eq!(added["marks"], 7);

    let listed = call("list_marks", json!({"document": id}));
    let marks = listed["marks"].as_array().unwrap();
    let kinds: Vec<&str> = marks.iter().map(|m| m["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        [
            "rect",
            "line",
            "text",
            "counter",
            "hide",
            "highlighter",
            "pen"
        ]
    );
    assert_eq!(marks[0]["color"], "#00AA00");
    assert_eq!(marks[0]["fill"], "#FFD60A");
    assert_eq!(marks[2]["text"], "Натисніть тут");
    assert_eq!(listed["picture"]["width"], 200);

    // Change: move the frame, recolour it, rewrite the text.
    call(
        "update_marks",
        json!({"document": id, "ids": [created[0]], "dx": 5, "dy": 5, "color": "blue", "fill": "none"}),
    );
    call(
        "update_marks",
        json!({"document": id, "ids": [created[2]], "text": "Сюди", "size": 22}),
    );
    let listed = call("list_marks", json!({"document": id}));
    let m = &listed["marks"];
    assert_eq!(
        (m[0]["x"].as_i64(), m[0]["y"].as_i64()),
        (Some(15), Some(15))
    );
    assert_eq!(m[0]["color"], "#3D7BF5");
    assert!(m[0].get("fill").is_none(), "{}", m[0]);
    assert_eq!(m[2]["text"], "Сюди");

    // An unknown id is said, nothing changes.
    let bad = e
        .agent
        .call("T", "delete_marks", &json!({"document": id, "ids": [9999]}));
    assert!(bad.is_error);
    let after = call("delete_marks", json!({"document": id, "ids": [created[6]]}));
    assert_eq!(after["marks"], 6);

    // The frame, a quarter turn, half the size, a tone — the size follows.
    let cropped = call(
        "crop",
        json!({"document": id, "x": 0, "y": 0, "width": 100, "height": 60}),
    );
    assert_eq!(
        (cropped["width"].as_i64(), cropped["height"].as_i64()),
        (Some(100), Some(60))
    );
    let whole = call("crop", json!({"document": id, "reset": true}));
    assert_eq!(whole["width"], 200);
    let turned = call("rotate", json!({"document": id, "turn": "right"}));
    assert_eq!(
        (turned["width"].as_i64(), turned["height"].as_i64()),
        (Some(120), Some(200))
    );
    call(
        "rotate",
        json!({"document": id, "turn": "left", "mirror": "horizontal"}),
    );
    let half = call("resize", json!({"document": id, "percent": 50}));
    assert_eq!(
        (half["width"].as_i64(), half["height"].as_i64()),
        (Some(100), Some(60))
    );
    let wide = call("resize", json!({"document": id, "width": 300}));
    assert_eq!(
        (wide["width"].as_i64(), wide["height"].as_i64()),
        (Some(300), Some(180))
    );
    call(
        "tone",
        json!({"document": id, "exposure": 0.5, "contrast": 10}),
    );
    call("tone", json!({"document": id, "reset": true}));
    let all = call("delete_marks", json!({"document": id, "all": true}));
    assert_eq!(all["marks"], 0);

    // The list of tools carries them, with the hints.
    let names: Vec<String> = crate::tools::list()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    for n in [
        "list_marks",
        "add_marks",
        "update_marks",
        "delete_marks",
        "crop",
        "rotate",
        "resize",
        "tone",
    ] {
        assert!(names.iter().any(|x| x == n), "{n} is listed");
    }
    let del = crate::tools::list()
        .into_iter()
        .find(|t| t["name"] == "delete_marks")
        .unwrap();
    assert_eq!(del["annotations"]["destructiveHint"], true);
}
