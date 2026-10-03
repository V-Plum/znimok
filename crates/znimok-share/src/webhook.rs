//! A webhook for anything else (n8n, Zapier, Make, a server of one's own): a POST of
//! `multipart/form-data` with two parts — `meta` (JSON: what it is) and `file` — and, when set,
//! one header whose value is a secret (`Authorization: Bearer …`, `X-Api-Key: …`). A check sends
//! a JSON `{"event": "test"}`. Any 2xx answer is a success; a JSON answer with `url` is where the
//! file went.

use serde_json::{Value, json};
use znimok_models::http::Transport;
use znimok_settings::WebhookTarget;

use crate::multipart::Form;
use crate::{Item, MAX_ANSWER, Sent, ShareError, status_error, timeout_for};

fn headers<'a>(w: &'a WebhookTarget, value: Option<&'a str>) -> Vec<(&'a str, &'a str)> {
    let mut h = vec![("User-Agent", "Znimok")];
    if let (false, Some(v)) = (w.header.trim().is_empty(), value) {
        h.push((w.header.trim(), v));
    }
    h
}

fn url_of(w: &WebhookTarget) -> Result<&str, ShareError> {
    let u = w.url.trim();
    if u.starts_with("https://")
        || u.starts_with("http://localhost")
        || u.starts_with("http://127.0.0.1")
    {
        Ok(u)
    } else {
        // Plain http would carry the file and the secret header in the open.
        Err(ShareError::Fail(
            "Webhook: the address must start with https://".into(),
        ))
    }
}

pub fn check(
    t: &dyn Transport,
    w: &WebhookTarget,
    value: Option<&str>,
    probe: &str,
) -> Result<String, ShareError> {
    let url = url_of(w)?;
    let body = json!({"event": "test", "app": "Znimok", "text": probe}).to_string();
    let r = t.request(
        "POST",
        url,
        &headers(w, value),
        Some((body.as_bytes(), "application/json")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    if !(200..300).contains(&r.status) {
        return Err(status_error("Webhook", &r));
    }
    Ok(format!("HTTP {}", r.status))
}

pub fn send(
    t: &dyn Transport,
    w: &WebhookTarget,
    value: Option<&str>,
    item: &Item,
    bytes: &[u8],
) -> Result<Sent, ShareError> {
    let url = url_of(w)?;
    let meta = json!({
        "event": "share",
        "app": "Znimok",
        "kind": item.kind,
        "title": item.title,
        "text": item.text,
        "file_name": item.file_name,
        "mime": item.mime,
        "size": bytes.len(),
    })
    .to_string();
    let (body, ct) = Form::new()
        .typed("meta", "application/json", meta.as_bytes())
        .file("file", &item.file_name, &item.mime, bytes)
        .finish();
    let r = t.request(
        "POST",
        url,
        &headers(w, value),
        Some((&body, &ct)),
        timeout_for(bytes.len()),
        MAX_ANSWER,
    )?;
    if !(200..300).contains(&r.status) {
        return Err(status_error("Webhook", &r));
    }
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    Ok(Sent {
        url: v["url"].as_str().map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Fake;

    fn hook(url: &str) -> WebhookTarget {
        WebhookTarget {
            id: "w".into(),
            enabled: true,
            name: "n8n".into(),
            url: url.into(),
            header: "Authorization".into(),
        }
    }

    #[test]
    fn meta_and_file_with_the_secret_header() {
        let f = Fake::new(&[(200, r#"{"url":"https://example.org/x/1"}"#)]);
        let item = Item {
            file_name: "a.mp4".into(),
            mime: "video/mp4".into(),
            title: "Запис".into(),
            kind: "video".into(),
            ..Default::default()
        };
        let s = send(
            &f,
            &hook("https://example.org/hook"),
            Some("Bearer s3"),
            &item,
            b"MP4",
        )
        .unwrap();
        assert_eq!(s.url.as_deref(), Some("https://example.org/x/1"));
        let r = &f.seen()[0];
        assert_eq!(r.header("Authorization"), Some("Bearer s3"));
        let b = r.body_text();
        assert!(b.contains("name=\"meta\"\r\nContent-Type: application/json"));
        assert!(b.contains("\"kind\":\"video\"") && b.contains("\"size\":3"));
        assert!(b.contains("name=\"file\"; filename=\"a.mp4\""));
    }

    #[test]
    fn plain_http_is_refused() {
        let f = Fake::new(&[]);
        let e = send(
            &f,
            &hook("http://example.org/hook"),
            None,
            &Item::default(),
            b"x",
        )
        .unwrap_err();
        assert!(!e.retry() && e.to_string().contains("https"));
        assert!(f.seen().is_empty());
    }
}
