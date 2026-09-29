//! The Ctrl+K assistant (ZK-73), by the owner's decisions of 28.09 (comment in ZK-8):
//!
//! - **What goes to the cloud without asking:** the request and the document's *structure* —
//!   size, name, tags, the marks (kind, box, text of text marks) and the windows that were on the
//!   screen, in screenshot pixels. **No pixels.** The model may ask to see the picture
//!   (`look_at_screenshot`): then [`Plan::needs_image`] is set and the app runs its «Надіслати
//!   знімок?» dialog (local OCR + masking, preview); with consent it calls [`Assistant::plan`]
//!   again with the prepared picture.
//! - **Plan first.** The model's tool calls are not executed: they become a [`Plan`] — editor
//!   commands (each one an undo step), and export / copy that need the person's confirmation. The
//!   app shows the plan; [`apply`] runs the editor part. Deleting library documents is not a tool.
//! - **Reading is local.** `query_document` calls are answered here from the editor, in a loop,
//!   until the model proposes changes or answers in words.
//! - **Cost.** Every round's tokens and estimated cost are summed into the plan (the app writes
//!   them to the agents' journal and the meter already counted them).
//!
//! Without a key or offline the app shows its local command palette instead; nothing here runs.

use schemars::schema_for;
use serde::Serialize;
use serde_json::{Value, json};
use znimok_core::{Command, Editor, Query};
use znimok_models::anthropic::{AiError, Client};
use znimok_models::image_prep::Prepared;
use znimok_models::pricing::Usage;

/// A window that was on the screen, in the screenshot's pixels — so «виділи вікно провідника»
/// becomes a frame exactly around it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Window {
    pub title: String,
    pub app: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// One step of a plan.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// One editor command = one undo step.
    Edit(Command),
    /// Needs the person's confirmation: data leaves the document.
    Export {
        format: String,
    },
    Copy,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// What the model says to the person (explanation or the answer to a question).
    pub message: String,
    /// The model wants to see the picture; ask for consent and plan again with it.
    pub needs_image: bool,
    /// Why, in the model's words (for the consent dialog).
    pub image_reason: String,
    pub usage: Usage,
    pub usd: Option<f64>,
    pub rounds: u32,
}

impl Plan {
    /// Export or copy: data leaves the document — confirm before running.
    pub fn needs_confirmation(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::Export { .. } | Step::Copy))
    }
}

/// Rounds of «read → answer» before the model must propose something.
const MAX_ROUNDS: u32 = 6;

pub struct Assistant<'a> {
    pub client: &'a Client,
    pub model: String,
    /// `YYYY-MM` for the spending meter.
    pub month: String,
    /// The person's interface language, so the model answers in it (`uk`, `en`).
    pub language: String,
}

/// A schema embedded inside a tool's input schema: its `$defs` must move to the root of that
/// input schema, where `#/$defs/…` references resolve.
fn embed(schema: schemars::Schema, defs: &mut serde_json::Map<String, Value>) -> Value {
    let mut v = serde_json::to_value(schema).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        if let Some(Value::Object(d)) = o.remove("$defs") {
            defs.extend(d);
        }
        o.remove("$schema");
    }
    v
}

fn with_defs(mut input: Value, defs: serde_json::Map<String, Value>) -> Value {
    if !defs.is_empty() {
        input["$defs"] = Value::Object(defs);
    }
    input
}

fn tools() -> Value {
    let mut edit_defs = serde_json::Map::new();
    let command = embed(schema_for!(Command), &mut edit_defs);
    let mut query_defs = serde_json::Map::new();
    let query = embed(schema_for!(Query), &mut query_defs);
    json!([
        {
            "name": "edit_document",
            "description": "Propose changes to the screenshot document as editor commands, in order. They are shown to the person as a plan and applied only after approval; each command is one undo step. Coordinates are screenshot pixels.",
            "input_schema": with_defs(json!({"type": "object", "properties": {
                "commands": {"type": "array", "items": command},
                "summary": {"type": "string", "description": "One short sentence for the plan, in the person's language"}
            }, "required": ["commands", "summary"]}), edit_defs)
        },
        {
            "name": "query_document",
            "description": "Read the document: its info, the list of marks, one mark, the selection, what is under a point or inside a rectangle. Answered immediately; changes nothing.",
            "input_schema": with_defs(json!({"type": "object", "properties": {
                "query": query
            }, "required": ["query"]}), query_defs)
        },
        {
            "name": "look_at_screenshot",
            "description": "Ask to see the picture itself. The person has to agree first (after secrets are hidden), so use it only when the structure and the windows are not enough — e.g. to find a button that is not a window.",
            "input_schema": {"type": "object", "properties": {
                "reason": {"type": "string", "description": "Why the picture is needed, for the person"}
            }, "required": ["reason"]}
        },
        {
            "name": "export",
            "description": "Propose exporting the result (the person confirms).",
            "input_schema": {"type": "object", "properties": {
                "format": {"type": "string", "enum": ["png", "jpeg", "webp", "html"]}
            }, "required": ["format"]}
        },
        {
            "name": "copy_to_clipboard",
            "description": "Propose copying the result to the clipboard (the person confirms).",
            "input_schema": {"type": "object", "properties": {}}
        }
    ])
}

/// The document as the model sees it: structure, no pixels.
pub fn structure(ed: &Editor, windows: &[Window]) -> Value {
    let doc = ed.query(&Query::GetDocument);
    let marks = ed.query(&Query::ListObjects);
    json!({"document": doc, "marks": marks, "windows": windows})
}

fn system_prompt(language: &str) -> String {
    format!(
        "You are the assistant inside Znimok, a screenshot editor. The person asks in plain words; \
you change their screenshot through tools. You get the document's structure (size, marks, the \
windows that were on screen with their boxes in screenshot pixels) — not the picture. Rules:\n\
- Propose edits with edit_document; nothing is applied until the person approves the plan.\n\
- To mark a window (\"виділи вікно провідника\"), use its box from `windows`: a rect mark around it.\n\
- Read with query_document when you need details; ask for the picture with look_at_screenshot only \
if the structure is not enough.\n\
- Export and copying only when asked, via their tools. You cannot delete library documents.\n\
- Answer and write mark texts in the person's language ({language}); never use Russian.\n\
- Be brief: one sentence of explanation."
    )
}

impl Assistant<'_> {
    /// Turns the request into a plan. `image`: the picture the person agreed to send (after
    /// masking), when a previous plan asked for it.
    pub fn plan(
        &self,
        request: &str,
        ed: &Editor,
        windows: &[Window],
        image: Option<&Prepared>,
    ) -> Result<Plan, AiError> {
        let mut first: Vec<Value> = Vec::new();
        if let Some(img) = image {
            first.push(json!({"type": "image", "source": {"type": "base64",
                "media_type": img.media_type, "data": img.base64}}));
        }
        first.push(json!({"type": "text", "text": format!(
            "Document structure:\n{}\n\nRequest: {request}",
            serde_json::to_string(&structure(ed, windows)).unwrap_or_default()
        )}));
        let mut messages = vec![json!({"role": "user", "content": first})];
        let mut plan = Plan::default();

        for round in 1..=MAX_ROUNDS {
            plan.rounds = round;
            let body = json!({
                "model": self.model,
                "max_tokens": 4096,
                "system": system_prompt(&self.language),
                "tools": tools(),
                "messages": messages,
            });
            let resp = self.client.messages(&body, &self.month)?;
            let u: Usage = serde_json::from_value(resp["usage"].clone()).unwrap_or_default();
            plan.usage.add(&u);
            let content = resp["content"].as_array().cloned().unwrap_or_default();
            let mut results = Vec::new();
            let mut proposed = false;
            for block in &content {
                match block["type"].as_str() {
                    Some("text") => {
                        if !plan.message.is_empty() {
                            plan.message.push('\n');
                        }
                        plan.message.push_str(block["text"].as_str().unwrap_or(""));
                    }
                    Some("tool_use") => {
                        let id = block["id"].clone();
                        let input = &block["input"];
                        let answer = match block["name"].as_str().unwrap_or("") {
                            "query_document" => {
                                match serde_json::from_value::<Query>(input["query"].clone()) {
                                    Ok(q) => {
                                        Ok(serde_json::to_string(&ed.query(&q)).unwrap_or_default())
                                    }
                                    Err(e) => Err(format!("invalid query: {e}")),
                                }
                            }
                            "edit_document" => {
                                let mut cmds = Vec::new();
                                let mut bad = None;
                                for (i, c) in input["commands"]
                                    .as_array()
                                    .cloned()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .enumerate()
                                {
                                    match serde_json::from_value::<Command>(c) {
                                        Ok(c) => cmds.push(c),
                                        Err(e) => {
                                            bad = Some(format!("command {i}: {e}"));
                                            break;
                                        }
                                    }
                                }
                                match bad {
                                    Some(e) => Err(e),
                                    None => {
                                        // Check them on a copy: the plan must apply cleanly.
                                        let mut trial = Editor::new(ed.doc.clone());
                                        match cmds
                                            .iter()
                                            .try_for_each(|c| trial.apply(c.clone()).map(|_| ()))
                                        {
                                            Err(e) => {
                                                Err(format!("the commands do not apply: {e}"))
                                            }
                                            Ok(()) => {
                                                if let Some(s) = input["summary"]
                                                    .as_str()
                                                    .filter(|s| !s.is_empty())
                                                {
                                                    if !plan.message.is_empty() {
                                                        plan.message.push('\n');
                                                    }
                                                    plan.message.push_str(s);
                                                }
                                                plan.steps.extend(cmds.into_iter().map(Step::Edit));
                                                proposed = true;
                                                Ok("proposed to the person".to_string())
                                            }
                                        }
                                    }
                                }
                            }
                            "look_at_screenshot" => {
                                plan.needs_image = true;
                                plan.image_reason =
                                    input["reason"].as_str().unwrap_or("").to_string();
                                proposed = true;
                                Ok("the person is asked".to_string())
                            }
                            "export" => {
                                plan.steps.push(Step::Export {
                                    format: input["format"].as_str().unwrap_or("png").to_string(),
                                });
                                proposed = true;
                                Ok("proposed to the person".to_string())
                            }
                            "copy_to_clipboard" => {
                                plan.steps.push(Step::Copy);
                                proposed = true;
                                Ok("proposed to the person".to_string())
                            }
                            other => Err(format!("unknown tool {other}")),
                        };
                        results.push(match answer {
                            Ok(t) => json!({"type": "tool_result", "tool_use_id": id, "content": t}),
                            Err(e) => json!({"type": "tool_result", "tool_use_id": id, "content": e, "is_error": true}),
                        });
                    }
                    _ => {}
                }
            }
            let reading_only = !results.is_empty() && !proposed;
            let had_errors = results.iter().any(|r| r["is_error"] == true);
            // Done: something proposed cleanly, or a plain answer without tools.
            if (proposed && !had_errors) || results.is_empty() {
                break;
            }
            if !(reading_only || had_errors) {
                break;
            }
            // Answer the reads / report the errors and let the model continue.
            messages.push(json!({"role": "assistant", "content": content}));
            messages.push(json!({"role": "user", "content": results}));
            if had_errors {
                plan.steps.clear();
                plan.needs_image = false;
            }
        }
        plan.usd = plan.usage.cost(&self.model);
        Ok(plan)
    }
}

/// Applies the editor part of an approved plan; export and copy stay with the app.
pub fn apply(plan: &Plan, ed: &mut Editor) -> Result<usize, String> {
    let mut n = 0;
    for s in &plan.steps {
        if let Step::Edit(c) = s {
            ed.apply(c.clone()).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests;
