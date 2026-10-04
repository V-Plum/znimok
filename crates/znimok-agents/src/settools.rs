//! Znimok's settings an agent may change (ZK-239): a white list, nothing else — the recording's
//! frame rate and quality, whether a recording opens in the editor, the DevTools log and the keys
//! it hides, the names of new documents, and the sound (turning sound on needs the separate sound
//! permission, as recording with it does). The same list is what `app_state` shows.
//!
//! With the app running the change goes through it (`agents.settings`), so the app saves it and
//! its window follows; without it, straight into `settings.json`, which the app reads on start.

use crate::permissions::Scope;
use crate::tools::{Agent, Output, Tool, obj};
use serde_json::{Value, json};
use znimok_settings::{Quality, Settings};

pub(crate) const TOOLS: &[Tool] = &[Tool {
    name: "settings",
    title: "Change Znimok's settings",
    description: "Changes a few of Znimok's settings — only these: fps (30 or 60) and quality (small, normal, high) of recordings, open_editor (a finished recording opens in the editor), devtools_log (keep the browser's log with a recording), hide_keys (the log's keys whose values are hidden; or hide_keys_add / hide_keys_remove), shot_prefix and video_prefix (the word before the date in new names; empty = the default), system_sound and microphone (in the person's recordings — turning one on needs the separate sound permission). Returns what changed and the values now; app_state shows them too.",
    scope: Some(Scope::Settings),
    read_only: false,
    schema: || {
        obj(
            json!({
                "fps": {"type": "integer", "enum": [30, 60]},
                "quality": {"type": "string", "enum": ["small", "normal", "high"]},
                "open_editor": {"type": "boolean"},
                "devtools_log": {"type": "boolean"},
                "hide_keys": {"type": "array", "items": {"type": "string"}, "description": "The whole list"},
                "hide_keys_add": {"type": "array", "items": {"type": "string"}},
                "hide_keys_remove": {"type": "array", "items": {"type": "string"}},
                "shot_prefix": {"type": "string", "maxLength": 40},
                "video_prefix": {"type": "string", "maxLength": 40},
                "system_sound": {"type": "boolean"},
                "microphone": {"type": "boolean"}
            }),
            &[],
        )
    },
}];

const KEYS: &[&str] = &[
    "fps",
    "quality",
    "open_editor",
    "devtools_log",
    "hide_keys",
    "hide_keys_add",
    "hide_keys_remove",
    "shot_prefix",
    "video_prefix",
    "system_sound",
    "microphone",
];

fn quality_name(q: Quality) -> &'static str {
    match q {
        Quality::Small => "small",
        Quality::Normal => "normal",
        Quality::High => "high",
    }
}

/// The white-listed settings as an agent sees them.
pub fn view(s: &Settings) -> Value {
    json!({
        "fps": s.video.fps,
        "quality": quality_name(s.video.quality),
        "open_editor": s.video.open_editor,
        "devtools_log": s.video.devtools_log,
        "hide_keys": s.video.hide_keys,
        "shot_prefix": s.library.shot_prefix,
        "video_prefix": s.library.video_prefix,
        "system_sound": s.video.audio.system,
        "microphone": s.video.audio.microphone,
    })
}

/// The change turns the sound of recordings on (system or microphone).
pub fn turns_sound_on(set: &Value) -> bool {
    set["system_sound"].as_bool() == Some(true) || set["microphone"].as_bool() == Some(true)
}

/// Checks a change against the white list: unknown keys and bad values are refused as a whole,
/// before anything is saved.
pub fn check(set: &Value) -> Result<(), String> {
    let o = set
        .as_object()
        .ok_or("the settings to change are an object")?;
    if o.is_empty() {
        return Err("nothing to change: give one of the listed settings".into());
    }
    if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!(
            "«{k}» is not a setting an agent may change (only: {})",
            KEYS.join(", ")
        ));
    }
    let mut probe = Settings::default();
    apply(&mut probe, set)
}

/// Applies a checked change. Errors on a bad value; the settings are then as before.
pub fn apply(s: &mut Settings, set: &Value) -> Result<(), String> {
    let mut next = s.clone();
    let boolean = |k: &str| -> Result<Option<bool>, String> {
        match &set[k] {
            Value::Null => Ok(None),
            Value::Bool(b) => Ok(Some(*b)),
            _ => Err(format!("«{k}» is true or false")),
        }
    };
    let text = |k: &str| -> Result<Option<String>, String> {
        match &set[k] {
            Value::Null => Ok(None),
            Value::String(t) if t.chars().count() <= 40 && !t.contains(['/', '\\', ':']) => {
                Ok(Some(t.trim().to_string()))
            }
            _ => Err(format!(
                "«{k}» is a short word (up to 40 characters, no / \\ :)"
            )),
        }
    };
    let list = |k: &str| -> Result<Option<Vec<String>>, String> {
        match &set[k] {
            Value::Null => Ok(None),
            Value::Array(a) => a
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::trim)
                        .filter(|t| !t.is_empty() && t.len() <= 100)
                        .map(str::to_string)
                        .ok_or_else(|| format!("«{k}»: keys are non-empty strings"))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            _ => Err(format!("«{k}» is a list of strings")),
        }
    };
    match &set["fps"] {
        Value::Null => {}
        v => match v.as_u64() {
            Some(f @ (30 | 60)) => next.video.fps = f as u32,
            _ => return Err("«fps» is 30 or 60".into()),
        },
    }
    match set["quality"].as_str() {
        None if set["quality"].is_null() => {}
        Some("small") => next.video.quality = Quality::Small,
        Some("normal") => next.video.quality = Quality::Normal,
        Some("high") => next.video.quality = Quality::High,
        _ => return Err("«quality» is small, normal or high".into()),
    }
    if let Some(b) = boolean("open_editor")? {
        next.video.open_editor = b;
    }
    if let Some(b) = boolean("devtools_log")? {
        next.video.devtools_log = b;
    }
    if let Some(b) = boolean("system_sound")? {
        next.video.audio.system = b;
    }
    if let Some(b) = boolean("microphone")? {
        next.video.audio.microphone = b;
    }
    if let Some(t) = text("shot_prefix")? {
        next.library.shot_prefix = t;
    }
    if let Some(t) = text("video_prefix")? {
        next.library.video_prefix = t;
    }
    if let Some(keys) = list("hide_keys")? {
        next.video.hide_keys = keys;
    }
    for k in list("hide_keys_add")?.unwrap_or_default() {
        if !next
            .video
            .hide_keys
            .iter()
            .any(|h| h.eq_ignore_ascii_case(&k))
        {
            next.video.hide_keys.push(k);
        }
    }
    for k in list("hide_keys_remove")?.unwrap_or_default() {
        next.video.hide_keys.retain(|h| !h.eq_ignore_ascii_case(&k));
    }
    *s = next;
    Ok(())
}

fn change(agent: &Agent, client: &str, args: &Value) -> Result<Output, String> {
    check(args)?;
    if turns_sound_on(args) {
        agent.authorize(client, Scope::RecordAudio, "settings")?;
    }
    let before = view(&agent.settings());
    let now = if agent.gui.call("app.state", json!({})).is_ok() {
        agent.gui.call("agents.settings", json!({"set": args}))?
    } else {
        let store = znimok_settings::Store::open(&agent.settings_file);
        let mut err = None;
        let saved = store
            .update(|s| {
                if let Err(e) = apply(s, args) {
                    err = Some(e);
                }
            })
            .map_err(|e| e.to_string())?;
        if let Some(e) = err {
            return Err(e);
        }
        view(&saved)
    };
    let changed: Vec<&str> = KEYS
        .iter()
        .copied()
        .filter(|k| before.get(k).is_some() && before[k] != now[k])
        .collect();
    Ok(Output::ok(
        json!({"changed": changed, "settings": now}),
        vec![],
    ))
}

/// `None`: not this module's tool.
pub(crate) fn run(
    agent: &Agent,
    client: &str,
    name: &str,
    args: &Value,
) -> Option<Result<Output, String>> {
    Some(match name {
        "settings" => change(agent, client, args),
        _ => return None,
    })
}
