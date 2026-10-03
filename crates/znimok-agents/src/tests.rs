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
        Ok(vec![WindowInfo {
            id: znimok_platform::WindowId(7),
            title: "Front window".into(),
            app: "test.exe".into(),
            pid: 1,
            bounds: Rect {
                x: 10,
                y: 10,
                width: 100,
                height: 60,
            },
            display: None,
            scale_factor: 1.0,
            minimized: false,
            own: false,
        }])
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
    assert_eq!(names[..3], ["list_targets", "capture", "record"]);
    assert!(
        list["result"].get("resultType").is_none(),
        "legacy results stay plain"
    );

    let shot = s
        .handle_line(&req(
            3,
            "tools/call",
            json!({"name": "capture",
            "arguments": {"target": "region", "x": 10, "y": 10, "width": 40, "height": 30}}),
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
            modern(json!({"name": "capture", "arguments": {"target": "screen"}})),
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

    // app_state needs no permission (list_targets does: window titles tell what the person does).
    let ds = s
        .handle_line(&req(
            5,
            "tools/call",
            modern(json!({"name": "app_state", "arguments": {}})),
        ))
        .unwrap();
    assert_eq!(ds["result"]["structuredContent"]["running"], false);

    let unknown = s
        .handle_line(&req(6, "sampling/createMessage", modern(json!({}))))
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
            modern(json!({"name": "list_targets", "arguments": {}})),
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
            modern(json!({"name": "capture", "arguments": {"target": "screen"}})),
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
            modern(json!({"name": "capture", "arguments": {"target": "screen"}})),
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
        "capture",
        json!({"target": "region", "x": 0, "y": 0, "width": 200, "height": 120}),
    );
    let id = shot["id"].as_str().unwrap().to_string();
    let added = call(
        "marks",
        json!({"document": id, "add": [
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
        "marks",
        json!({"document": id, "update": [
            {"ids": [created[0]], "dx": 5, "dy": 5, "color": "blue", "fill": "none"},
            {"ids": [created[2]], "text": "Сюди", "size": 22},
        ]}),
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
    let bad = e.agent.call(
        "T",
        "marks",
        &json!({"document": id, "delete": {"ids": [9999]}}),
    );
    assert!(bad.is_error);
    let after = call(
        "marks",
        json!({"document": id, "delete": {"ids": [created[6]]}}),
    );
    assert_eq!(after["marks"], 6);

    // The frame, a quarter turn, half the size, a tone — the size follows.
    let cropped = call(
        "transform",
        json!({"document": id, "crop": {"x": 0, "y": 0, "width": 100, "height": 60}}),
    );
    assert_eq!(
        (cropped["width"].as_i64(), cropped["height"].as_i64()),
        (Some(100), Some(60))
    );
    let whole = call(
        "transform",
        json!({"document": id, "crop": {"reset": true}}),
    );
    assert_eq!(whole["width"], 200);
    let turned = call(
        "transform",
        json!({"document": id, "rotate": {"turn": "right"}}),
    );
    assert_eq!(
        (turned["width"].as_i64(), turned["height"].as_i64()),
        (Some(120), Some(200))
    );
    // Several steps in one call, in the tool's order: a turn and a mirror, then half the size.
    let half = call(
        "transform",
        json!({"document": id, "rotate": {"turn": "left", "mirror": "horizontal"}, "resize": {"percent": 50}}),
    );
    assert_eq!(
        (half["width"].as_i64(), half["height"].as_i64()),
        (Some(100), Some(60))
    );
    let wide = call(
        "transform",
        json!({"document": id, "resize": {"width": 300}}),
    );
    assert_eq!(
        (wide["width"].as_i64(), wide["height"].as_i64()),
        (Some(300), Some(180))
    );
    call(
        "transform",
        json!({"document": id, "tone": {"exposure": 0.5, "contrast": 10}}),
    );
    call(
        "transform",
        json!({"document": id, "tone": {"reset": true}}),
    );
    let all = call("marks", json!({"document": id, "delete": {"all": true}}));
    assert_eq!(all["marks"], 0);

    // The list of tools carries them, with the hints.
    let names: Vec<String> = crate::tools::list()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    for n in ["list_marks", "marks", "transform"] {
        assert!(names.iter().any(|x| x == n), "{n} is listed");
    }
    // The published list is the facade's: one tool per job, 21 of them, the finer ones hidden.
    assert_eq!(names.len(), 21, "{names:?}");
    for n in [
        "add_marks",
        "crop",
        "capture_region",
        "record_start",
        "devlog_get",
    ] {
        assert!(!names.iter().any(|x| x == n), "{n} is not published");
        assert!(
            e.agent.call("T", n, &json!({})).is_error,
            "{n} is not callable"
        );
    }
    let del = crate::tools::list()
        .into_iter()
        .find(|t| t["name"] == "library_delete")
        .unwrap();
    assert_eq!(del["annotations"]["destructiveHint"], true);
    let mk = crate::tools::list()
        .into_iter()
        .find(|t| t["name"] == "marks")
        .unwrap();
    assert_eq!(mk["annotations"]["destructiveHint"], false);
}

/// ZK-234: name and meta, a picture brought in, a copy, the trash and back, the tags, the
/// filters — and deleting for good refused without the person's yes.
#[test]
fn library_tools_meta_import_trash() {
    let e = env("libtools", true);
    for s in [Scope::Capture, Scope::LibraryRead, Scope::LibraryWrite] {
        e.agent.perms.grant("T", s, Grant::Always).unwrap();
    }
    let call = |name: &str, args: Value| {
        let out = e.agent.call("T", name, &args);
        assert!(!out.is_error, "{name}: {:?}", out.content);
        out.structured.unwrap()
    };
    let shot = call(
        "capture",
        json!({"target": "region", "x": 0, "y": 0, "width": 64, "height": 40}),
    );
    let id = shot["id"].as_str().unwrap().to_string();

    let m = call(
        "set_meta",
        json!({"document": id, "name": "Вікно входу", "description": "Помилка після кліку",
               "tags": ["bug", "login"], "pinned": true}),
    );
    assert_eq!(m["name"], "Вікно входу");
    assert_eq!(m["tags"], json!(["bug", "login"]));
    assert_eq!(m["pinned"], true);
    let m = call(
        "set_meta",
        json!({"document": id, "add_tags": ["Bug", "ui"], "remove_tags": ["login"]}),
    );
    assert_eq!(
        m["tags"],
        json!(["bug", "ui"]),
        "no duplicate by case, one removed"
    );

    // A picture file from outside comes in as a new document.
    let png = e.dir.join("outside.png");
    let r = Raster::solid(30, 20, Rgb::new(10, 20, 30));
    std::fs::write(&png, crate::library::encode_png(&r).unwrap()).unwrap();
    let imp = call(
        "library_edit",
        json!({"action": "import", "path": png.display().to_string()}),
    );
    assert_eq!(imp["name"], "outside");
    assert_eq!(
        (imp["width"].as_i64(), imp["height"].as_i64()),
        (Some(30), Some(20))
    );
    let imported = imp["id"].as_str().unwrap().to_string();

    let copy = call(
        "library_edit",
        json!({"action": "duplicate", "document": id}),
    );
    assert_ne!(copy["id"], json!(id));
    assert_eq!(copy["tags"], json!(["bug", "ui"]));
    assert_eq!(copy["pinned"], false, "a copy is not pinned");

    let tags = call("library_search", json!({"with_tags": true}));
    assert_eq!(tags["tags"][0], json!({"tag": "bug", "documents": 2}));

    // The filters.
    let pinned = call("library_search", json!({"pinned": true}));
    assert_eq!(pinned["documents"].as_array().unwrap().len(), 1);
    assert_eq!(pinned["documents"][0]["kind"], "screenshot");
    let none = call("library_search", json!({"kind": "video"}));
    assert!(none["documents"].as_array().unwrap().is_empty());
    let old = call("library_search", json!({"until": "2020-01-01"}));
    assert!(old["documents"].as_array().unwrap().is_empty());
    let bad = e
        .agent
        .call("T", "library_search", &json!({"since": "yesterday"}));
    assert!(bad.is_error);

    // The trash: no question, and back.
    let t = call(
        "library_edit",
        json!({"action": "trash", "document": imported}),
    );
    assert_eq!(t["trashed"], true);
    assert_eq!(
        call("library_search", json!({}))["documents"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let in_trash = call("library_search", json!({"trash": true}));
    assert_eq!(in_trash["documents"][0]["id"], json!(imported));
    let back = call(
        "library_edit",
        json!({"action": "restore", "document": imported}),
    );
    assert_eq!(back["id"], json!(imported));
    assert_eq!(
        call("library_search", json!({}))["documents"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    // For good: nobody to say yes (no app) — refused, the file stays.
    let del = e
        .agent
        .call("T", "library_delete", &json!({"document": imported}));
    assert!(del.is_error, "{:?}", del.content);
    assert_eq!(
        call("library_search", json!({}))["documents"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let listed = crate::tools::list();
    let d = listed
        .iter()
        .find(|t| t["name"] == "library_delete")
        .unwrap();
    assert_eq!(d["annotations"]["destructiveHint"], true);
}

/// ZK-235: a recording read from its file — what it is, its browser log in short and in
/// detail, one event whole; a screenshot is told apart; the log is a resource.
#[test]
fn recording_info_and_devtools_log() {
    use znimok_format::video::{DevEvent, DevLog, MouseButton, MouseEvent};
    let e = env("vidtools", true);
    e.agent
        .perms
        .grant("T", Scope::LibraryRead, Grant::Always)
        .unwrap();
    e.agent
        .perms
        .grant("T", Scope::Capture, Grant::Always)
        .unwrap();
    std::fs::create_dir_all(&e.agent.lib.dir).unwrap();
    let doc =
        znimok_core::Document::from_raster("Запис із логом", Raster::solid(160, 100, Rgb::WHITE));
    let mut video = znimok_format::Video::new(znimok_format::VideoInfo {
        width: 160,
        height: 100,
        fps_milli: 30_000,
        frames: 90,
        duration_hns: 30_000_000,
        codec: znimok_format::video::CODEC_H264,
    });
    let ev = |ms, json: &str| DevEvent {
        ms,
        json: json.into(),
    };
    video.devlog = Some(DevLog {
        wall0_ms: 0,
        events: vec![
            ev(100, r#"{"k":"nav","s":0,"url":"https://example.org/form"}"#),
            ev(
                900,
                r#"{"k":"net","s":0,"method":"POST","url":"https://example.org/api/save","status":500,"dur":42,"reqHeaders":{"Authorization":"Bearer abc.def.ghi"},"body":"{\"error\":\"boom\"}"}"#,
            ),
            ev(
                1500,
                r#"{"k":"dl","s":0,"ev":"purchase","data":{"event":"purchase","value":42}}"#,
            ),
            ev(
                2000,
                r#"{"k":"error","s":2,"text":"TypeError: form is undefined","src":"https://example.org/app.js:40"}"#,
            ),
            ev(
                2100,
                r#"{"k":"console","s":1,"lvl":"warning","text":"deprecated"}"#,
            ),
        ],
    });
    video.mouse = vec![
        MouseEvent {
            ms: 500,
            x: 40,
            y: 30,
            button: MouseButton::Left,
            down: true,
        },
        MouseEvent {
            ms: 560,
            x: 40,
            y: 30,
            button: MouseButton::Left,
            down: false,
        },
    ];
    let mp4: Vec<u8> = (0..2048u32).map(|i| (i * 7) as u8).collect();
    let bytes = znimok_format::write_video(&doc, &video, &mp4, &Default::default());
    let path = e.agent.lib.dir.join("Znimok-test-video.znimok");
    std::fs::write(&path, bytes).unwrap();
    let id = doc.id.to_string();
    let call = |name: &str, args: Value| {
        let out = e.agent.call("T", name, &args);
        assert!(!out.is_error, "{name}: {:?}", out.content);
        out.structured.unwrap()
    };

    let found = call("library_search", json!({"has_log": true}));
    assert_eq!(found["documents"][0]["kind"], "video");
    assert_eq!(found["documents"][0]["duration_ms"], 3000);

    let info = call("video_info", json!({"document": id}));
    assert_eq!(info["duration_ms"], 3000);
    assert_eq!(info["fps"], 30.0);
    assert_eq!(
        info["clicks"],
        json!([{"at_ms": 500, "x": 40, "y": 30, "button": "left"}])
    );
    assert_eq!(info["devtools_log"]["events"], 5);

    let sum = call("devlog", json!({"document": id}));
    assert_eq!(sum["events"], 5);
    assert_eq!(sum["by_kind"]["network"], 1);
    assert_eq!(sum["errors"][0]["text"], "TypeError: form is undefined");
    assert_eq!(sum["errors"][0]["at_ms"], 2000);
    assert_eq!(sum["failed_requests"][0]["status"], 500);
    assert_eq!(sum["datalayer_events"]["purchase"], 1);
    assert_eq!(sum["clicks"], 1);

    let errs = call("devlog", json!({"document": id, "errors_only": true}));
    assert_eq!(
        errs["matching"], 2,
        "the failed request and the error: {errs}"
    );
    let net = call("devlog", json!({"document": id, "kinds": ["network"]}));
    let i = net["events"][0]["i"].as_u64().unwrap();
    assert!(net["events"][0].get("body").is_none(), "rows are short");
    let one = call("devlog", json!({"document": id, "index": i}));
    assert_eq!(one["event"]["body"], "{\"error\":\"boom\"}");
    // The token of the request is hidden (the settings' default: hide).
    let auth = one["event"]["reqHeaders"]["Authorization"]
        .as_str()
        .unwrap();
    assert!(!auth.contains("abc.def.ghi"), "{auth}");
    let window = call(
        "devlog",
        json!({"document": id, "from_ms": 1000, "to_ms": 2050}),
    );
    assert_eq!(window["matching"], 2);
    let q = call("devlog", json!({"document": id, "query": "PURCHASE"}));
    assert_eq!(q["events"][0]["kind"], "datalayer");

    // A recording is not exported as a picture in silence (ZK-250): said plainly.
    let ex = e
        .agent
        .call("T", "export", &json!({"document": id, "format": "png"}));
    assert!(ex.is_error, "{:?}", ex.content);
    assert!(
        ex.content[0]["text"]
            .as_str()
            .unwrap()
            .contains("recording")
    );

    // A screenshot is not a recording.
    let shot = call(
        "capture",
        json!({"target": "region", "x": 0, "y": 0, "width": 40, "height": 30}),
    );
    let not = e
        .agent
        .call("T", "video_info", &json!({"document": shot["id"]}));
    assert!(not.is_error);

    // The log as a resource.
    let res = e.agent.resources("T").unwrap();
    assert!(
        res.iter()
            .any(|r| r["uri"] == format!("znimok://library/{id}/log"))
    );
    let log = e
        .agent
        .read_resource("T", &format!("znimok://library/{id}/log"))
        .unwrap();
    assert_eq!(log["mimeType"], "application/json");
    assert!(log["text"].as_str().unwrap().contains("example.org/form"));
}

/// ZK-236: the ready scenarios are listed and filled in; every tool they name exists; the hints
/// tell a read from a change.
#[test]
fn prompts_and_hints() {
    let e = env("prompts", true);
    let mut s = Server::new(&e.agent);
    let disc = s
        .handle_line(&req(1, "server/discover", modern(json!({}))))
        .unwrap();
    assert!(
        disc["result"]["capabilities"]["prompts"].is_object(),
        "{disc}"
    );
    let list = s
        .handle_line(&req(2, "prompts/list", modern(json!({}))))
        .unwrap();
    let names: Vec<&str> = list["result"]["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "bug_report",
            "document_screen",
            "redact_before_sharing",
            "read_recording"
        ]
    );
    let got = s
        .handle_line(&req(
            3,
            "prompts/get",
            modern(json!({"name": "bug_report", "arguments": {"problem": "кнопка не натискається", "window": "Chrome"}})),
        ))
        .unwrap();
    let text = got["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(
        text.contains("кнопка не натискається") && text.contains("«Chrome»"),
        "{text}"
    );
    assert!(!text.contains('{'), "every placeholder is filled: {text}");
    let none = s
        .handle_line(&req(4, "prompts/get", modern(json!({"name": "nope"}))))
        .unwrap();
    assert_eq!(none["error"]["code"], -32602);

    let tools = crate::tools::list();
    for t in crate::prompts::named_tools() {
        assert!(
            tools.iter().any(|x| x["name"] == t.as_str()),
            "a prompt names «{t}», which is not a tool"
        );
    }
    let hint =
        |n: &str, k: &str| tools.iter().find(|t| t["name"] == n).unwrap()["annotations"][k].clone();
    assert_eq!(hint("list_marks", "readOnlyHint"), true);
    assert_eq!(hint("list_marks", "idempotentHint"), true);
    assert_eq!(hint("marks", "readOnlyHint"), false);
    assert_eq!(hint("library_delete", "destructiveHint"), true);
    assert_eq!(hint("library_edit", "destructiveHint"), false);
    for t in &tools {
        assert_eq!(t["annotations"]["openWorldHint"], false, "{}", t["name"]);
    }
    // annotate offers the document's commands only (ZK-250): no grants, settings or captures.
    let ann = tools.iter().find(|t| t["name"] == "annotate").unwrap();
    let cmds: Vec<&str> = ann["inputSchema"]["properties"]["commands"]["items"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            v["properties"]["cmd"]["const"]
                .as_str()
                .or_else(|| v["properties"]["cmd"]["enum"][0].as_str())
                .unwrap_or("")
        })
        .collect();
    assert!(cmds.contains(&"add_object"), "{cmds:?}");
    for bad in [
        "agent_grant",
        "agent_revoke",
        "set_setting",
        "library_delete",
        "capture",
        "export",
        "share",
    ] {
        assert!(!cmds.contains(&bad), "{bad} is not an annotate command");
    }
}

/// ZK-237: recording goes to the app over IPC; sound is a permission of its own, asked before
/// anything starts; stop returns the library document.
#[test]
fn recording_through_the_app() {
    use std::sync::{Arc, Mutex};
    let mut e = env("rec", true);
    let cfg = znimok_ipc::Config {
        suffix: Some(format!("r{}", std::process::id())),
        // Short: the path of a Unix socket has a small limit, and macOS temp folders are long.
        dir: Some(std::env::temp_dir().join(format!("zkr{}", std::process::id()))),
        ..Default::default()
    };
    e.agent.gui = Gui::with_config(cfg.clone());
    // Without the app: refused with a reason.
    e.agent
        .perms
        .grant("T", Scope::Record, Grant::Always)
        .unwrap();
    let out = e.agent.call("T", "record", &json!({"action": "start"}));
    assert!(out.is_error, "no app, no recording");

    // The document «the app» will return.
    std::fs::create_dir_all(&e.agent.lib.dir).unwrap();
    let doc = znimok_core::Document::from_raster("Запис агента", Raster::solid(64, 48, Rgb::WHITE));
    let video = znimok_format::Video::new(znimok_format::VideoInfo {
        width: 64,
        height: 48,
        fps_milli: 30_000,
        frames: 60,
        duration_hns: 20_000_000,
        codec: znimok_format::video::CODEC_H264,
    });
    let path = e.agent.lib.dir.join("Znimok-agent-rec.znimok");
    std::fs::write(
        &path,
        znimok_format::write_video(&doc, &video, &[7u8; 512], &Default::default()),
    )
    .unwrap();

    let seen: Arc<Mutex<Vec<Value>>> = Default::default();
    let (seen2, path2) = (seen.clone(), path.display().to_string());
    let _s = znimok_ipc::Server::start(cfg, move |m: &str, p: Value| match m {
        "agents.ask" => Ok(json!({"grant": if p["scope"] == "record_audio" { Value::Null } else { json!("once") }})),
        "agents.activity" => Ok(Value::Null),
        "agents.record" => {
            seen2.lock().unwrap().push(p.clone());
            match p["op"].as_str() {
                Some("start") => Ok(json!({"recording": true, "width": 800, "height": 600})),
                Some("stop") => Ok(json!({"path": path2})),
                Some("status") => Ok(json!({"recording": false, "finished": path2})),
                _ => Ok(json!({"recording": true, "paused": p["op"] == "pause"})),
            }
        }
        _ => Err(znimok_ipc::RpcError::method_not_found(m)),
    })
    .unwrap();

    // Sound: the person refuses its permission → nothing reaches the recorder.
    let out = e
        .agent
        .call("T", "record", &json!({"action": "start", "sound": "both"}));
    assert!(out.is_error);
    assert!(
        seen.lock().unwrap().is_empty(),
        "refused before the app is asked to record"
    );

    let out = e.agent.call(
        "T",
        "record",
        &json!({"action": "start", "window": 42, "limit_seconds": 20, "devtools_log": false}),
    );
    assert!(!out.is_error, "{:?}", out.content);
    let sent = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(sent["window"], 42);
    assert_eq!(sent["limit_s"], 20);
    assert_eq!(sent["log"], false);
    assert_eq!(sent["system_audio"], false);
    assert_eq!(sent["microphone"], false);

    let out = e.agent.call("T", "record", &json!({"action": "pause"}));
    assert_eq!(out.structured.unwrap()["paused"], true);
    let out = e.agent.call("T", "record", &json!({"action": "stop"}));
    assert!(!out.is_error, "{:?}", out.content);
    let got = out.structured.unwrap();
    assert_eq!(got["id"], doc.id.to_string());
    assert_eq!(got["kind"], "video");
    assert_eq!(got["duration_ms"], 2000);
    let st = e
        .agent
        .call("T", "record_status", &json!({}))
        .structured
        .unwrap();
    assert_eq!(st["finished"]["id"], doc.id.to_string());

    // A client without the permission and nobody saying yes... the fake app says «once» — so
    // check the scope instead: the tools belong to `record`.
    let tools = crate::tools::list();
    for n in ["record", "record_status"] {
        assert!(tools.iter().any(|t| t["name"] == n), "{n}");
    }
}

/// ZK-238: the window in front, a document opened / copied through the app, the app's state.
#[test]
fn active_window_and_the_app() {
    let mut e = env("apptools", true);
    let cfg = znimok_ipc::Config {
        suffix: Some(format!("a{}", std::process::id())),
        // Short: the path of a Unix socket has a small limit, and macOS temp folders are long.
        dir: Some(std::env::temp_dir().join(format!("zka{}", std::process::id()))),
        ..Default::default()
    };
    e.agent.gui = Gui::with_config(cfg.clone());
    e.agent
        .perms
        .grant("T", Scope::Capture, Grant::Always)
        .unwrap();
    e.agent
        .perms
        .grant("T", Scope::LibraryRead, Grant::Always)
        .unwrap();

    let out = e
        .agent
        .call("T", "capture", &json!({"target": "active_window"}));
    assert!(!out.is_error, "{:?}", out.content);
    let shot = out.structured.unwrap();
    assert!(shot["window"]["title"].is_string(), "{shot}");
    let id = shot["id"].as_str().unwrap().to_string();

    // No app: the state says so, the others are refused.
    let st = e
        .agent
        .call("T", "app_state", &json!({}))
        .structured
        .unwrap();
    assert_eq!(st["running"], false);
    assert!(
        e.agent
            .call("T", "hand_over", &json!({"document": id, "to": "editor"}))
            .is_error
    );

    let seen: std::sync::Arc<std::sync::Mutex<Vec<Value>>> = Default::default();
    let seen2 = seen.clone();
    let _s = znimok_ipc::Server::start(cfg, move |m: &str, p: Value| match m {
        "agents.activity" => Ok(Value::Null),
        "app.state" => Ok(json!({"page": "editor"})),
        "agents.app" => {
            seen2.lock().unwrap().push(p.clone());
            Ok(json!({"ok": p["op"]}))
        }
        _ => Err(znimok_ipc::RpcError::method_not_found(m)),
    })
    .unwrap();
    let st = e
        .agent
        .call("T", "app_state", &json!({}))
        .structured
        .unwrap();
    assert_eq!(
        (st["running"].clone(), st["page"].clone()),
        (json!(true), json!("editor"))
    );
    for (to, op) in [("editor", "open"), ("clipboard", "copy")] {
        let out = e
            .agent
            .call("T", "hand_over", &json!({"document": id, "to": to}));
        assert!(!out.is_error, "hand_over {to}: {:?}", out.content);
        let sent = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(sent["op"], op);
        assert!(sent["path"].as_str().unwrap().ends_with(".znimok"));
    }
}

/// ZK-251: one question per client — «this session» for one scope covers the others, but not
/// the sound of a recording; «this time» covers nothing more.
#[test]
fn one_question_covers_every_scope_but_sound() {
    use std::sync::{Arc, Mutex};
    for (grant, all, asks_expected) in [("session", true, 1usize), ("once", false, 2)] {
        let mut e = env(&format!("oneq{grant}"), true);
        let cfg = znimok_ipc::Config {
            suffix: Some(format!("q{grant}{}", std::process::id())),
            dir: Some(std::env::temp_dir().join(format!(
                "zkq{}{}",
                &grant[..1],
                std::process::id()
            ))),
            ..Default::default()
        };
        e.agent.gui = Gui::with_config(cfg.clone());
        let asked: Arc<Mutex<Vec<String>>> = Default::default();
        let asked2 = asked.clone();
        let _s = znimok_ipc::Server::start(cfg, move |m: &str, p: Value| match m {
            "agents.ask" => {
                asked2
                    .lock()
                    .unwrap()
                    .push(p["scope"].as_str().unwrap_or("").to_string());
                if p["scope"] == "record_audio" {
                    Ok(json!({"grant": Value::Null, "all": false}))
                } else {
                    Ok(json!({"grant": grant, "all": all}))
                }
            }
            "agents.activity" => Ok(Value::Null),
            _ => Err(znimok_ipc::RpcError::method_not_found(m)),
        })
        .unwrap();
        // A capture asks; a library read after it asks again only when the answer was «once».
        let shot = e.agent.call("T", "capture", &json!({"target": "screen"}));
        assert!(!shot.is_error, "{:?}", shot.content);
        let found = e.agent.call("T", "library_search", &json!({}));
        assert!(!found.is_error, "{:?}", found.content);
        assert_eq!(
            asked.lock().unwrap().len(),
            asks_expected,
            "{grant}: {:?}",
            asked.lock().unwrap()
        );
        // Sound is never part of «everything».
        assert_eq!(
            e.agent.perms.check("T", Scope::RecordAudio),
            crate::permissions::Decision::Ask
        );
        let rec = e.agent.call(
            "T",
            "record",
            &json!({"action": "start", "sound": "system"}),
        );
        assert!(rec.is_error);
        assert_eq!(
            asked.lock().unwrap().last().map(String::as_str),
            Some("record_audio")
        );
    }
}
