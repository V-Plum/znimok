//! End to end through the command layer (ZK-115), on both OSes in CI, with the synthetic OS as
//! the screen: capture → marks → export → library, over MCP exactly as an agent calls it.
//!
//! Pixels are checked exactly where they are known (a capture is a crop of the synthetic screen,
//! and stays so outside the marks); the export must equal the saved document as rendered, and the
//! library resource must equal the export. How each mark looks is znimok-render's golden tests
//! (ZK-114) — freezing it here would break on every design change.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use znimok_agents::audit::Audit;
use znimok_agents::backend::{FromPlatform, Gui};
use znimok_agents::library::Library;
use znimok_agents::mcp::{MODERN, Server};
use znimok_agents::permissions::{Grant, Permissions, Scope};
use znimok_agents::tools::Agent;
use znimok_platform::synthetic::SyntheticOs;

const CLIENT: &str = "e2e";

struct Run {
    agent: Agent,
    dir: PathBuf,
    next: u64,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Run {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("zk-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let os = SyntheticOs::windows_like().platform();
        let perms = Permissions::new(dir.join("agents.json"));
        for s in [Scope::Capture, Scope::LibraryRead, Scope::LibraryWrite] {
            perms.grant(CLIENT, s, Grant::Always).unwrap();
        }
        let agent = Agent {
            lib: Library {
                dir: dir.join("lib"),
            },
            perms,
            audit: Some(Audit::open(dir.join("audit.jsonl"))),
            gui: Gui::with_config(znimok_ipc::Config {
                suffix: Some(format!("e2e{}", std::process::id())),
                dir: Some(dir.join("ipc")),
                ..Default::default()
            }),
            capture: Box::new(FromPlatform {
                capture: os.capture.clone(),
                windows: os.windows.clone(),
            }),
            enabled: true,
            export_dir: dir.join("exports"),
        };
        Self {
            agent,
            dir,
            next: 1,
        }
    }

    /// One MCP tool call (modern protocol); panics on a tool error with its message.
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let msg = json!({"jsonrpc": "2.0", "id": self.next, "method": "tools/call", "params": {
            "name": name, "arguments": args,
            "_meta": {"io.modelcontextprotocol/protocolVersion": MODERN,
                      "io.modelcontextprotocol/clientInfo": {"name": CLIENT, "version": "1"}}}});
        self.next += 1;
        let r = Server::new(&self.agent)
            .handle_line(&msg.to_string())
            .unwrap();
        let res = r["result"].clone();
        assert_eq!(
            res["isError"], false,
            "{name}: {}",
            res["content"][0]["text"]
        );
        res
    }

    fn read(&mut self, method: &str, params: Value) -> Value {
        let mut p = params;
        p["_meta"] = json!({"io.modelcontextprotocol/protocolVersion": MODERN,
                            "io.modelcontextprotocol/clientInfo": {"name": CLIENT}});
        let msg = json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": p});
        self.next += 1;
        Server::new(&self.agent)
            .handle_line(&msg.to_string())
            .unwrap()["result"]
            .clone()
    }
}

fn rgba_of(path: &Path) -> image::RgbaImage {
    let doc = znimok_agents::library::load(path).unwrap();
    let r = znimok_agents::library::render(&doc, 1.0);
    image::RgbaImage::from_raw(r.width, r.height, r.rgba).unwrap()
}

/// Share of pixels differing by more than 3 in any channel, and the largest difference.
fn diff(a: &image::RgbaImage, b: &image::RgbaImage) -> (f64, u8) {
    assert_eq!(a.dimensions(), b.dimensions());
    let mut bad = 0usize;
    let mut max = 0u8;
    for (p, q) in a.pixels().zip(b.pixels()) {
        let d = (0..4).map(|i| p[i].abs_diff(q[i])).max().unwrap();
        max = max.max(d);
        if d > 3 {
            bad += 1;
        }
    }
    (bad as f64 / (a.width() * a.height()) as f64, max)
}

#[test]
fn capture_annotate_export_library() {
    let mut run = Run::new();

    // 1. A region of the primary display: exactly the synthetic screen's pixels.
    let displays = run.tool("list_targets", json!({}))["structuredContent"]["displays"].clone();
    assert_eq!(displays[0]["primary"], true);
    let shot = run.tool(
        "capture",
        json!({"target": "region", "x": 100, "y": 50, "width": 320, "height": 200}),
    );
    let s = &shot["structuredContent"];
    assert_eq!(
        (s["width"].as_u64(), s["height"].as_u64()),
        (Some(320), Some(200))
    );
    let id = s["id"].as_str().unwrap().to_string();
    let path = PathBuf::from(s["path"].as_str().unwrap());
    let px = rgba_of(&path);
    for (x, y) in [(0u32, 0u32), (319, 0), (0, 199), (123, 77)] {
        let bgra = SyntheticOs::expected_pixel(0, 100 + x, 50 + y);
        assert_eq!(
            px.get_pixel(x, y).0,
            [bgra[2], bgra[1], bgra[0], 255],
            "pixel {x},{y}"
        );
    }
    let kinds: Vec<&str> = shot["content"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["text", "image", "resource_link"]);

    // 2. A window: filled with its synthetic colour.
    let windows = run.tool("list_targets", json!({}))["structuredContent"]["windows"].clone();
    let wid = windows[0]["id"].as_u64().unwrap();
    let wshot = run.tool("capture", json!({"target": "window", "window": wid}));
    let wpath = PathBuf::from(wshot["structuredContent"]["path"].as_str().unwrap());
    let wbgra = SyntheticOs::window_pixel(znimok_platform::WindowId(wid));
    assert_eq!(
        rgba_of(&wpath).get_pixel(5, 5).0,
        [wbgra[2], wbgra[1], wbgra[0], 255]
    );

    // 3. Marks on the region shot, then export.
    let ann = run.tool("annotate", json!({"document": id, "commands": [
        {"cmd": "add_object", "object": {"rect": {"x": 20, "y": 20, "w": 120, "h": 70}, "data": {"kind": "rect"}}},
        {"cmd": "add_object", "object": {"rect": {"x": 170, "y": 30, "w": 120, "h": 80}, "data": {"kind": "ellipse"}}},
        {"cmd": "add_object", "object": {"rect": {"x": 30, "y": 170, "w": 250, "h": -60},
            "data": {"kind": "line", "head_front": "triangle", "head_back": "none", "head_size": 1}}},
        {"cmd": "add_object", "object": {"rect": {"x": 200, "y": 130, "w": 100, "h": 50}, "data": {"kind": "hide", "mode": "pixelate", "strength": 60}}},
        {"cmd": "add_object", "object": {"rect": {"x": 40, "y": 110, "w": 110, "h": 24}, "data": {"kind": "mark"}}}
    ]}));
    assert_eq!(ann["structuredContent"]["marks"], 5);
    let out = run.dir.join("e2e.png");
    run.tool(
        "export",
        json!({"document": id, "format": "png", "path": out.display().to_string()}),
    );
    let exported = image::open(&out).unwrap().into_rgba8();
    assert_eq!(exported.dimensions(), (320, 200));

    // The export is the saved document as the renderer draws it (the look of each mark is
    // znimok-render's business and its own golden tests, ZK-114 — not frozen here).
    let saved = rgba_of(&path);
    let (share, max) = diff(&exported, &saved);
    assert!(
        share < 0.005,
        "export differs from the saved document: {:.2}%, max {max}",
        share * 100.0
    );
    // Outside every mark: exactly the captured screen.
    for (x, y) in [(310u32, 5u32), (5, 5), (315, 150)] {
        let bgra = SyntheticOs::expected_pixel(0, 100 + x, 50 + y);
        assert_eq!(
            exported.get_pixel(x, y).0,
            [bgra[2], bgra[1], bgra[0], 255],
            "untouched {x},{y}"
        );
    }
    // Under the marks: changed (frame edge, ellipse edge, hidden area).
    let changed = |x: u32, y: u32| {
        let bgra = SyntheticOs::expected_pixel(0, 100 + x, 50 + y);
        exported.get_pixel(x, y).0 != [bgra[2], bgra[1], bgra[0], 255]
    };
    // Near an edge (where exactly the stroke lies is the renderer's choice).
    let near = |x: u32, y: u32| (x - 6..=x + 6).any(|xx| changed(xx, y));
    assert!(near(20, 50), "the frame is drawn");
    assert!(near(170, 70), "the ellipse is drawn");
    let hidden = (130..180)
        .flat_map(|y| (200..300).map(move |x| (x, y)))
        .filter(|&(x, y)| changed(x, y))
        .count();
    assert!(
        hidden > 2500,
        "the hidden area is changed: {hidden} of 5000 pixels"
    );

    // 4. The library sees both documents; get and resources agree with the export.
    let found = run.tool("library_search", json!({}))["structuredContent"]["documents"].clone();
    assert_eq!(found.as_array().unwrap().len(), 2);
    let got = run.tool("library_get", json!({"document": &id[..8]}));
    assert_eq!(got["structuredContent"]["marks"], 5);
    let res = run.read(
        "resources/read",
        json!({"uri": format!("znimok://library/{id}")}),
    );
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD
        .decode(res["contents"][0]["blob"].as_str().unwrap())
        .unwrap();
    let from_resource = image::load_from_memory(&png).unwrap().into_rgba8();
    assert!(
        diff(&from_resource, &exported).0 < 0.005,
        "resource and export differ"
    );

    // 5. Masking and OCR run (the synthetic screen has no text); the journal saw it all.
    let red = run.tool(
        "redact_pii",
        json!({"document": id, "apply": false, "faces": false}),
    );
    assert_eq!(red["structuredContent"]["applied"], false);
    let journal = run.agent.audit.as_ref().unwrap().entries();
    assert!(
        journal.iter().filter(|e| e.capture && e.ok).count() == 2,
        "{journal:?}"
    );
    assert!(journal.iter().all(|e| e.ok && e.client == CLIENT));
}
