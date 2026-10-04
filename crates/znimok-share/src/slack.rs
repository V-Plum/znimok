//! Slack through a bot token (`xoxb-…`, a Slack app with the `files:write` and `chat:write`
//! scopes, added to the channel): the upload in Slack's three steps — an upload address, the
//! bytes, then the file shared in the channel with the words as its comment.

use serde_json::{Value, json};
use znimok_models::http::Transport;

use crate::multipart::percent;
use crate::{Item, MAX_ANSWER, Sent, ShareError, status_error, timeout_for};

const API: &str = "https://slack.com/api";

fn answer(r: &znimok_models::http::Response) -> Result<Value, ShareError> {
    if r.status != 200 {
        return Err(status_error("Slack", r));
    }
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    if v["ok"] == Value::Bool(true) {
        return Ok(v);
    }
    let code = v["error"].as_str().unwrap_or("unknown_error");
    Err(match code {
        "ratelimited" | "internal_error" | "service_unavailable" | "fatal_error" => {
            ShareError::Again(format!("Slack: {code}"))
        }
        "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive" => {
            ShareError::Fail("Slack: the bot token is not accepted".into())
        }
        "channel_not_found" | "not_in_channel" => ShareError::Fail(format!(
            "Slack: {code} — the channel's ID, and the app added to it"
        )),
        _ => ShareError::Fail(format!("Slack: {code}")),
    })
}

/// «Create the Slack app» (ZK-272): Slack's page of a new app from a manifest, everything filled
/// in — the name, the bot, the two scopes. The person picks the workspace and presses Create.
pub fn app_url() -> String {
    let manifest = json!({
        "display_information": {"name": "Znimok", "description": "Screenshots and recordings from Znimok"},
        "features": {"bot_user": {"display_name": "Znimok", "always_online": false}},
        "oauth_config": {"scopes": {"bot": ["files:write", "chat:write"]}},
        "settings": {"org_deploy_enabled": false, "socket_mode_enabled": false, "token_rotation_enabled": false}
    });
    format!(
        "https://api.slack.com/apps?new_app=1&manifest_json={}",
        percent(&manifest.to_string())
    )
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// The workspace and the bot (`auth.test`).
pub fn check(t: &dyn Transport, token: &str, channel: &str) -> Result<String, ShareError> {
    let a = bearer(token);
    let r = t.request(
        "POST",
        &format!("{API}/auth.test"),
        &[("Authorization", &a)],
        Some((b"", "application/x-www-form-urlencoded")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    let v = answer(&r)?;
    let mut out = format!(
        "{} · {}",
        v["team"].as_str().unwrap_or("?"),
        v["user"].as_str().unwrap_or("?")
    );
    if !channel.trim().is_empty() {
        out = format!("{out} → {}", channel.trim());
    }
    Ok(out)
}

pub fn send(
    t: &dyn Transport,
    token: &str,
    channel: &str,
    item: &Item,
    bytes: &[u8],
) -> Result<Sent, ShareError> {
    if channel.trim().is_empty() {
        return Err(ShareError::Fail("Slack: no channel in the settings".into()));
    }
    let a = bearer(token);
    // 1. Where to put the bytes.
    let url = format!(
        "{API}/files.getUploadURLExternal?filename={}&length={}",
        percent(&item.file_name),
        bytes.len()
    );
    let r = t.request(
        "GET",
        &url,
        &[("Authorization", &a)],
        None,
        timeout_for(0),
        MAX_ANSWER,
    )?;
    let v = answer(&r)?;
    let upload = v["upload_url"]
        .as_str()
        .ok_or_else(|| ShareError::Fail("Slack: no upload address".into()))?
        .to_string();
    let file_id = v["file_id"].as_str().unwrap_or("").to_string();
    // 2. The bytes.
    let r = t.request(
        "POST",
        &upload,
        &[],
        Some((bytes, &item.mime)),
        timeout_for(bytes.len()),
        MAX_ANSWER,
    )?;
    if !(200..300).contains(&r.status) {
        return Err(status_error("Slack", &r));
    }
    // 3. Shared in the channel.
    let mut comment = item.title.clone();
    if !item.text.trim().is_empty() {
        comment = format!("{comment}\n{}", item.text.trim());
    }
    let body = json!({
        "files": [{"id": file_id, "title": item.title}],
        "channel_id": channel.trim(),
        "initial_comment": comment,
    })
    .to_string();
    let r = t.request(
        "POST",
        &format!("{API}/files.completeUploadExternal"),
        &[("Authorization", &a)],
        Some((body.as_bytes(), "application/json; charset=utf-8")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    let v = answer(&r)?;
    Ok(Sent {
        url: v["files"][0]["permalink"].as_str().map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Fake;

    #[test]
    fn three_steps() {
        let f = Fake::new(&[
            (
                200,
                r#"{"ok":true,"upload_url":"https://files.slack.com/upload/v1/abc","file_id":"F1"}"#,
            ),
            (200, "OK - 3"),
            (
                200,
                r#"{"ok":true,"files":[{"id":"F1","permalink":"https://x.slack.com/files/F1"}]}"#,
            ),
        ]);
        let item = Item {
            file_name: "Знімок.png".into(),
            mime: "image/png".into(),
            title: "Знімок".into(),
            ..Default::default()
        };
        let s = send(&f, "xoxb-1", "C123", &item, b"PNG").unwrap();
        assert_eq!(s.url.as_deref(), Some("https://x.slack.com/files/F1"));
        let seen = f.seen();
        assert!(
            seen[0]
                .url
                .contains("files.getUploadURLExternal?filename=%D0%97")
                && seen[0].url.ends_with("&length=3")
        );
        assert_eq!(seen[0].header("Authorization"), Some("Bearer xoxb-1"));
        assert_eq!(seen[1].url, "https://files.slack.com/upload/v1/abc");
        assert_eq!(seen[1].body, b"PNG");
        let v: Value = serde_json::from_slice(&seen[2].body).unwrap();
        assert_eq!(v["channel_id"], "C123");
        assert_eq!(v["files"][0]["id"], "F1");
    }

    #[test]
    fn the_app_from_a_manifest() {
        let u = app_url();
        assert!(u.starts_with("https://api.slack.com/apps?new_app=1&manifest_json=%7B"));
        assert!(u.contains("files%3Awrite") && u.contains("chat%3Awrite"));
        assert!(!u.contains(' ') && !u.contains('"'));
    }

    #[test]
    fn slack_errors() {
        let f = Fake::new(&[(200, r#"{"ok":false,"error":"not_in_channel"}"#)]);
        let item = Item::default();
        let e = send(&f, "xoxb-1", "C1", &item, b"x").unwrap_err();
        assert!(!e.retry() && e.to_string().contains("not_in_channel"));
        let f = Fake::new(&[(200, r#"{"ok":false,"error":"ratelimited"}"#)]);
        assert!(send(&f, "xoxb-1", "C1", &item, b"x").unwrap_err().retry());
    }
}
