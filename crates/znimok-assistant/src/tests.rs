use super::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use znimok_core::{Document, Raster, Rgb};
use znimok_models::http::{HttpError, Response, Transport};

/// Answers with the scripted bodies in order and keeps what was sent.
struct Script {
    replies: Mutex<Vec<Value>>,
    sent: Mutex<Vec<Value>>,
}

struct Shared(Arc<Script>);

impl Transport for Shared {
    fn post_json(
        &self,
        _: &str,
        _: &[(&str, &str)],
        body: &[u8],
        _: Duration,
    ) -> Result<Response, HttpError> {
        self.0
            .sent
            .lock()
            .unwrap()
            .push(serde_json::from_slice(body).unwrap());
        let r = self.0.replies.lock().unwrap().remove(0);
        Ok(Response {
            status: 200,
            body: r.to_string().into_bytes(),
            retry_after: None,
        })
    }
}

fn reply(content: Value) -> Value {
    json!({"model": "claude-sonnet-5-5", "stop_reason": "tool_use", "content": content,
           "usage": {"input_tokens": 1000, "output_tokens": 100}})
}

fn tool(id: &str, name: &str, input: Value) -> Value {
    json!({"type": "tool_use", "id": id, "name": name, "input": input})
}

fn setup(replies: Vec<Value>) -> (Arc<Script>, Client, Editor) {
    let s = Arc::new(Script {
        replies: Mutex::new(replies),
        sent: Mutex::new(Vec::new()),
    });
    let client = Client::new("sk-test".into(), Box::new(Shared(s.clone())), None);
    let ed = Editor::new(Document::from_raster(
        "t",
        Raster::solid(800, 600, Rgb::WHITE),
    ));
    (s, client, ed)
}

fn assistant(client: &Client) -> Assistant<'_> {
    Assistant {
        client,
        model: "claude-sonnet-5-5".into(),
        month: "2026-09".into(),
        language: "uk".into(),
    }
}

fn explorer() -> Vec<Window> {
    vec![Window {
        title: "Документи".into(),
        app: "explorer.exe".into(),
        x: 40,
        y: 30,
        w: 500,
        h: 400,
    }]
}

const FRAME: &str = r#"{"cmd":"add_object","object":{"rect":{"x":40,"y":30,"w":500,"h":400},"data":{"kind":"rect"}}}"#;

#[test]
fn frame_the_explorer_window_from_structure_only() {
    let (s, client, mut ed) = setup(vec![reply(json!([
        {"type": "text", "text": "Готово."},
        tool("t1", "edit_document", json!({"commands": [serde_json::from_str::<Value>(FRAME).unwrap()],
                                          "summary": "Рамка навколо вікна Провідника"}))
    ]))]);
    let plan = assistant(&client)
        .plan("виділи вікно провідника", &ed, &explorer(), None)
        .unwrap();
    assert_eq!(plan.steps.len(), 1);
    assert!(!plan.needs_confirmation() && !plan.needs_image);
    assert!(plan.message.contains("Рамка навколо вікна Провідника"));
    assert_eq!(plan.rounds, 1);
    // No pixels went out; the windows did.
    let sent = s.sent.lock().unwrap();
    let first = &sent[0]["messages"][0]["content"];
    assert!(
        first
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["type"] != "image")
    );
    assert!(first[0]["text"].as_str().unwrap().contains("explorer.exe"));
    assert!(
        sent[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "edit_document")
    );
    assert!(
        sent[0]["system"]
            .as_str()
            .unwrap()
            .contains("never use Russian")
    );
    drop(sent);
    assert!(ed.doc.objects.is_empty(), "nothing applied before approval");
    assert_eq!(apply(&plan, &mut ed).unwrap(), 1);
    assert_eq!(ed.doc.objects.len(), 1);
    // The plan's cost is known (1000 in / 100 out on Sonnet 5.5).
    assert!((plan.usd.unwrap() - (1000.0 * 2.0 + 100.0 * 10.0) / 1e6).abs() < 1e-12);
}

#[test]
fn reads_are_answered_locally_then_the_plan_comes() {
    let (s, client, ed) = setup(vec![
        reply(json!([tool(
            "q1",
            "query_document",
            json!({"query": {"query": "list_objects"}})
        )])),
        reply(json!([tool(
            "t1",
            "edit_document",
            json!({"commands": [serde_json::from_str::<Value>(FRAME).unwrap()], "summary": "Рамка"})
        )])),
    ]);
    let plan = assistant(&client)
        .plan("обведи вікно", &ed, &explorer(), None)
        .unwrap();
    assert_eq!(plan.rounds, 2);
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.usage.input_tokens, 2000, "both rounds counted");
    let sent = s.sent.lock().unwrap();
    let second = &sent[1]["messages"];
    assert_eq!(second[1]["role"], "assistant");
    let result = &second[2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "q1");
    assert!(result["content"].as_str().unwrap().contains("objects"));
}

#[test]
fn a_broken_command_goes_back_to_the_model_and_is_fixed() {
    let (s, client, ed) = setup(vec![
        reply(json!([tool(
            "t1",
            "edit_document",
            json!({"commands": [{"cmd": "fly_away"}], "summary": "?"})
        )])),
        reply(json!([tool(
            "t2",
            "edit_document",
            json!({"commands": [serde_json::from_str::<Value>(FRAME).unwrap()], "summary": "Рамка"})
        )])),
    ]);
    let plan = assistant(&client).plan("обведи", &ed, &[], None).unwrap();
    assert_eq!(plan.steps.len(), 1);
    let sent = s.sent.lock().unwrap();
    assert_eq!(sent[1]["messages"][2]["content"][0]["is_error"], true);
}

#[test]
fn the_picture_only_after_consent() {
    let (s, client, ed) = setup(vec![
        reply(json!([tool(
            "i1",
            "look_at_screenshot",
            json!({"reason": "Потрібно знайти кнопку «Зберегти»"})
        )])),
        reply(json!([{"type": "text", "text": "Кнопка праворуч унизу."}])),
    ]);
    let a = assistant(&client);
    let plan = a.plan("де кнопка зберегти?", &ed, &[], None).unwrap();
    assert!(plan.needs_image && plan.steps.is_empty());
    assert_eq!(plan.image_reason, "Потрібно знайти кнопку «Зберегти»");
    // The app asked, the person agreed; the masked picture goes along now.
    let img = znimok_models::image_prep::prepare(
        &znimok_models::Rgba::new(8, 8, vec![255; 256]).unwrap(),
    )
    .unwrap();
    let plan = a.plan("де кнопка зберегти?", &ed, &[], Some(&img)).unwrap();
    assert_eq!(plan.message, "Кнопка праворуч унизу.");
    let sent = s.sent.lock().unwrap();
    assert_eq!(sent[1]["messages"][0]["content"][0]["type"], "image");
}

#[test]
fn export_and_copy_need_confirmation() {
    let (_s, client, mut ed) = setup(vec![reply(json!([
        tool("e1", "export", json!({"format": "png"})),
        tool("c1", "copy_to_clipboard", json!({}))
    ]))]);
    let plan = assistant(&client)
        .plan("збережи й скопіюй", &ed, &[], None)
        .unwrap();
    assert!(plan.needs_confirmation());
    assert_eq!(
        plan.steps,
        vec![
            Step::Export {
                format: "png".into()
            },
            Step::Copy
        ]
    );
    assert_eq!(
        apply(&plan, &mut ed).unwrap(),
        0,
        "the editor part is empty"
    );
}

/// Every `$ref` in a tool schema points into that tool's own `$defs` (refs of an embedded schema
/// resolve from the root of the input schema).
#[test]
fn tool_schemas_resolve_their_references() {
    fn refs(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(o) => {
                if let Some(r) = o.get("$ref").and_then(Value::as_str) {
                    out.push(r.to_string());
                }
                o.values().for_each(|x| refs(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| refs(x, out)),
            _ => {}
        }
    }
    for t in tools().as_array().unwrap() {
        let schema = &t["input_schema"];
        let mut r = Vec::new();
        refs(schema, &mut r);
        for x in r {
            let name = x.strip_prefix("#/$defs/").unwrap_or_else(|| panic!("{x}"));
            assert!(
                schema["$defs"].get(name).is_some(),
                "{}: {x} does not resolve",
                t["name"]
            );
        }
        assert!(
            schema.get("$schema").is_none()
                && schema["properties"]
                    .to_string()
                    .find("\"$schema\"")
                    .is_none()
        );
    }
}
