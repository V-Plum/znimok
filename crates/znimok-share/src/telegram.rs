//! Telegram through a bot the user made with @BotFather: the file as a document (sendDocument —
//! the picture as it is, not recompressed), the words as its caption.

use serde_json::Value;
use znimok_models::http::Transport;

use crate::multipart::Form;
use crate::{Item, MAX_ANSWER, Sent, ShareError, status_error, timeout_for};

const API: &str = "https://api.telegram.org";
/// The Bot API takes files up to 50 MB.
const MAX_FILE: usize = 50 << 20;
/// A caption is at most 1024 characters.
const MAX_CAPTION: usize = 1024;

fn call(
    t: &dyn Transport,
    token: &str,
    method: &str,
    body: Option<(&[u8], &str)>,
    bytes: usize,
) -> Result<Value, ShareError> {
    let url = format!("{API}/bot{token}/{method}");
    let r = match body {
        Some(b) => t.request("POST", &url, &[], Some(b), timeout_for(bytes), MAX_ANSWER)?,
        None => t.request("GET", &url, &[], None, timeout_for(0), MAX_ANSWER)?,
    };
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    if r.status == 200 && v["ok"] == Value::Bool(true) {
        return Ok(v["result"].clone());
    }
    // The Bot API says why in `description`; the token never goes into a message.
    let why = v["description"].as_str().unwrap_or("").to_string();
    let e = status_error("Telegram", &r);
    Err(match (r.status, e) {
        (401, _) => ShareError::Fail("Telegram: the bot token is not valid".into()),
        (_, ShareError::Again(_)) => ShareError::Again(format!("Telegram: {why}")),
        _ => ShareError::Fail(format!("Telegram: {why}")),
    })
}

/// The bot, the chat, and a short message in it.
pub fn check(
    t: &dyn Transport,
    token: &str,
    chat: &str,
    probe: &str,
) -> Result<String, ShareError> {
    let me = call(t, token, "getMe", None, 0)?;
    let bot = me["username"].as_str().unwrap_or("bot").to_string();
    if chat.trim().is_empty() {
        return Ok(format!("@{bot}"));
    }
    let body = serde_json::json!({ "chat_id": chat.trim(), "text": probe }).to_string();
    call(
        t,
        token,
        "sendMessage",
        Some((body.as_bytes(), "application/json")),
        0,
    )?;
    Ok(format!("@{bot} → {}", chat.trim()))
}

/// The chats that wrote to the bot lately (its updates), newest first: `(id, name)`. The user
/// writes anything to the bot, then picks the chat here instead of looking its id up.
pub fn find_chats(t: &dyn Transport, token: &str) -> Result<Vec<(String, String)>, ShareError> {
    let updates = call(t, token, "getUpdates", None, 0)?;
    let mut out: Vec<(String, String)> = Vec::new();
    for u in updates.as_array().into_iter().flatten().rev() {
        let m = [
            "message",
            "channel_post",
            "my_chat_member",
            "edited_message",
        ]
        .iter()
        .find_map(|k| u.get(*k))
        .and_then(|m| m.get("chat"));
        let Some(chat) = m else { continue };
        let Some(id) = chat["id"].as_i64() else {
            continue;
        };
        let name = chat["title"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                let f = chat["first_name"].as_str().unwrap_or("");
                let l = chat["last_name"].as_str().unwrap_or("");
                Some(format!("{f} {l}").trim().to_string()).filter(|s| !s.is_empty())
            })
            .or_else(|| chat["username"].as_str().map(|u| format!("@{u}")))
            .unwrap_or_default();
        let id = id.to_string();
        if !out.iter().any(|(i, _)| *i == id) {
            out.push((id, name));
        }
    }
    Ok(out)
}

pub fn send(
    t: &dyn Transport,
    token: &str,
    chat: &str,
    item: &Item,
    bytes: &[u8],
) -> Result<Sent, ShareError> {
    if chat.trim().is_empty() {
        return Err(ShareError::Fail("Telegram: no chat in the settings".into()));
    }
    if bytes.len() > MAX_FILE {
        return Err(ShareError::Fail(format!(
            "Telegram: the file is {} MB, a bot can send up to 50 MB",
            bytes.len() >> 20
        )));
    }
    let mut caption = item.title.clone();
    if !item.text.trim().is_empty() {
        caption = format!("{caption}\n{}", item.text.trim());
    }
    let caption: String = caption.chars().take(MAX_CAPTION).collect();
    let (body, ct) = Form::new()
        .text("chat_id", chat.trim())
        .text("caption", &caption)
        .file("document", &item.file_name, &item.mime, bytes)
        .finish();
    let msg = call(t, token, "sendDocument", Some((&body, &ct)), bytes.len())?;
    // A public channel has an address of its message; a private chat has none.
    let url = msg["chat"]["username"]
        .as_str()
        .zip(msg["message_id"].as_i64())
        .map(|(u, id)| format!("https://t.me/{u}/{id}"));
    Ok(Sent { url })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Fake;

    fn item() -> Item {
        Item {
            file_name: "Знімок.png".into(),
            mime: "image/png".into(),
            title: "Знімок".into(),
            text: "кнопка не працює".into(),
            kind: "screenshot".into(),
        }
    }

    #[test]
    fn sends_a_document_with_its_caption() {
        let f = Fake::new(&[(
            200,
            r#"{"ok":true,"result":{"message_id":7,"chat":{"id":-100,"username":"chan"}}}"#,
        )]);
        let s = send(&f, "123:abc", "-100", &item(), b"PNG").unwrap();
        assert_eq!(s.url.as_deref(), Some("https://t.me/chan/7"));
        let r = &f.seen()[0];
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "https://api.telegram.org/bot123:abc/sendDocument");
        assert!(r.content_type.starts_with("multipart/form-data"));
        let b = r.body_text();
        assert!(b.contains("name=\"chat_id\"\r\n\r\n-100"));
        assert!(b.contains("Знімок\nкнопка не працює"));
        assert!(b.contains("name=\"document\""));
    }

    #[test]
    fn a_bad_token_and_a_busy_service() {
        let f = Fake::new(&[(401, r#"{"ok":false,"description":"Unauthorized"}"#)]);
        let e = send(&f, "1:x", "5", &item(), b"x").unwrap_err();
        assert!(!e.retry() && e.to_string().contains("not valid"), "{e}");
        assert!(!e.to_string().contains("1:x"));
        let f = Fake::new(&[(
            429,
            r#"{"ok":false,"description":"Too Many Requests: retry after 3"}"#,
        )]);
        assert!(send(&f, "1:x", "5", &item(), b"x").unwrap_err().retry());
    }

    #[test]
    fn chats_from_the_updates() {
        let f = Fake::new(&[(
            200,
            r#"{"ok":true,"result":[
                {"message":{"chat":{"id":32364506,"first_name":"Вадим","last_name":"Слива"}}},
                {"my_chat_member":{"chat":{"id":-1002,"title":"Звіти"}}},
                {"message":{"chat":{"id":32364506,"first_name":"Вадим","last_name":"Слива"}}}
            ]}"#,
        )]);
        let chats = find_chats(&f, "1:x").unwrap();
        assert_eq!(
            chats,
            [
                ("32364506".to_string(), "Вадим Слива".to_string()),
                ("-1002".to_string(), "Звіти".to_string())
            ]
        );
    }
}
