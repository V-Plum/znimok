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
        "oauth_config": {"scopes": {"bot": ["files:write", "chat:write", "channels:read", "groups:read"]}},
        "settings": {"org_deploy_enabled": false, "socket_mode_enabled": false, "token_rotation_enabled": false}
    });
    format!(
        "https://api.slack.com/apps?new_app=1&manifest_json={}",
        percent(&manifest.to_string())
    )
}

// ------------------------------------------------------------------ signing in (ZK-273)

/// Where the answer of a sign-in comes on this computer (Atlassian's registered address too).
pub const SIGN_IN_PORT: u16 = 47821;
pub const SIGN_IN_PATH: &str = "/callback";
/// The address Slack sends the person back to — registered in Znimok's Slack app: Slack wants
/// https for an app other workspaces may install, so the site's page hands the code on to
/// `http://localhost:47821/callback` (site/oauth/slack.html).
pub const SIGN_IN_RELAY: &str = "https://v-plum.github.io/znimok/oauth/slack.html";
/// User scopes: a desktop sign-in with PKCE may not ask for a bot, so Znimok posts as the person —
/// files into a channel, the channels to choose from.
pub const USER_SCOPES: &str = "files:write,chat:write,channels:read,groups:read";
/// What a stored token starts with when it is a sign-in's refresh token (else a pasted one).
pub const SIGNED_IN: &str = "oauth:";

/// The client id of Znimok's Slack app (PKCE, a public client: no secret), given at build time
/// (`ZNIMOK_SLACK_CLIENT_ID`); a build without it has no «Sign in to Slack».
pub fn client_id() -> Option<&'static str> {
    option_env!("ZNIMOK_SLACK_CLIENT_ID")
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Starts «Sign in to Slack»: the address to open, and the port waiting for the answer.
pub fn sign_in(client_id: &str) -> std::io::Result<znimok_google::Pending> {
    znimok_google::begin_via(
        "https://slack.com/oauth/v2/authorize",
        &[("client_id", client_id), ("user_scope", USER_SCOPES)],
        Some(SIGN_IN_PORT),
        SIGN_IN_PATH,
        Some(SIGN_IN_RELAY),
    )
}

/// A sign-in's result: the workspace's name and what to keep (the refresh token, marked).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    pub team: String,
    pub keep: String,
}

fn form_call(t: &dyn Transport, pairs: &[(&str, &str)]) -> Result<Value, ShareError> {
    let body = znimok_google::form_body(pairs);
    let r = t.request(
        "POST",
        &format!("{API}/oauth.v2.access"),
        &[],
        Some((body.as_bytes(), "application/x-www-form-urlencoded")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    answer(&r)
}

/// The user's token in an answer: under `authed_user` at a sign-in, at the top at a refresh.
fn user_tokens(v: &Value) -> (Option<String>, Option<String>) {
    let u = if v["authed_user"]["access_token"].is_string() {
        &v["authed_user"]
    } else {
        v
    };
    (
        u["access_token"].as_str().map(str::to_string),
        u["refresh_token"].as_str().map(str::to_string),
    )
}

/// The code for the person's token (PKCE: no secret).
pub fn exchange(
    t: &dyn Transport,
    client_id: &str,
    code: &znimok_google::Code,
) -> Result<Signed, ShareError> {
    let v = form_call(
        t,
        &[
            ("client_id", client_id),
            ("code", &code.code),
            ("code_verifier", code.verifier()),
            ("redirect_uri", code.redirect()),
        ],
    )?;
    let (access, refresh) = user_tokens(&v);
    let keep = match (refresh, access) {
        (Some(r), _) => format!("{SIGNED_IN}{r}"),
        (None, Some(a)) => a,
        (None, None) => return Err(ShareError::Fail("Slack gave no token".into())),
    };
    Ok(Signed {
        team: v["team"]["name"].as_str().unwrap_or("Slack").to_string(),
        keep,
    })
}

/// A fresh token for a signed-in account, and the refresh token to keep instead of the old one
/// (Slack turns it over at each refresh).
pub fn refresh(
    t: &dyn Transport,
    client_id: &str,
    refresh_token: &str,
) -> Result<(String, String), ShareError> {
    let v = form_call(
        t,
        &[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
    )
    .map_err(|e| match e {
        ShareError::Fail(m) => ShareError::Fail(format!("{m} — sign in to Slack again")),
        again => again,
    })?;
    match user_tokens(&v) {
        (Some(a), r) => Ok((a, r.unwrap_or_else(|| refresh_token.to_string()))),
        _ => Err(ShareError::Fail(
            "Slack gave no token — sign in again".into(),
        )),
    }
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// The channels the bot can post to (ZK-278): the public ones and the private ones it is in,
/// `(id, #name)`, by name. Needs `channels:read` / `groups:read` (the app made from the manifest
/// has them; an older app says so).
pub fn channels(t: &dyn Transport, token: &str) -> Result<Vec<(String, String)>, ShareError> {
    let a = bearer(token);
    let mut out = Vec::new();
    let mut cursor = String::new();
    for _ in 0..10 {
        let mut url = format!(
            "{API}/conversations.list?types=public_channel,private_channel&exclude_archived=true&limit=200"
        );
        if !cursor.is_empty() {
            url.push_str(&format!("&cursor={}", percent(&cursor)));
        }
        let r = t.request(
            "GET",
            &url,
            &[("Authorization", &a)],
            None,
            timeout_for(0),
            MAX_ANSWER,
        )?;
        let v = match answer(&r) {
            Err(ShareError::Fail(m)) if m.contains("missing_scope") => {
                return Err(ShareError::Fail(
                    "Slack: the app may not list channels — make it again with «Create the Slack app» or type the channel's ID".into(),
                ));
            }
            other => other?,
        };
        for c in v["channels"].as_array().into_iter().flatten() {
            if let (Some(id), Some(name)) = (c["id"].as_str(), c["name"].as_str()) {
                out.push((id.to_string(), format!("#{name}")));
            }
        }
        cursor = v["response_metadata"]["next_cursor"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if cursor.is_empty() {
            break;
        }
    }
    out.sort_by_key(|p| p.1.to_lowercase());
    Ok(out)
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
    fn channels_by_name_over_pages() {
        let f = Fake::new(&[
            (
                200,
                r#"{"ok":true,"channels":[{"id":"C2","name":"zeta"}],"response_metadata":{"next_cursor":"abc="}}"#,
            ),
            (
                200,
                r#"{"ok":true,"channels":[{"id":"C1","name":"Alpha"}],"response_metadata":{"next_cursor":""}}"#,
            ),
        ]);
        let c = channels(&f, "xoxb").unwrap();
        assert_eq!(
            c,
            [
                ("C1".to_string(), "#Alpha".to_string()),
                ("C2".into(), "#zeta".into())
            ]
        );
        assert!(f.seen()[1].url.ends_with("&cursor=abc%3D"));
        let f = Fake::new(&[(200, r#"{"ok":false,"error":"missing_scope"}"#)]);
        assert!(
            channels(&f, "xoxb")
                .unwrap_err()
                .to_string()
                .contains("Create the Slack app")
        );
    }

    #[test]
    fn sign_in_exchange_and_refresh() {
        let p = sign_in("123.456").unwrap();
        // Slack sends the person to the site's https page, which hands the code on.
        assert_eq!(p.redirect, SIGN_IN_RELAY);
        assert!(
            p.url.contains(
                "redirect_uri=https%3A%2F%2Fv-plum.github.io%2Fznimok%2Foauth%2Fslack.html"
            )
        );
        assert!(p.url.contains("user_scope=files%3Awrite%2Cchat%3Awrite"));
        assert!(!p.url.contains("client_secret"));
        drop(p);
        let f = Fake::new(&[(
            200,
            r#"{"ok":true,"team":{"name":"Plum Co"},"authed_user":{"id":"U1","access_token":"xoxe.xoxp-1","refresh_token":"xoxe-1-r1","expires_in":43200}}"#,
        )]);
        // A code as the loopback gives it.
        let p = sign_in("123.456").unwrap();
        let state = p.url.split("state=").nth(1).unwrap().to_string();
        let h = std::thread::spawn(move || {
            use std::io::Write;
            let mut s = std::net::TcpStream::connect("127.0.0.1:47821").unwrap();
            write!(
                s,
                "GET /callback?state={state}&code=c1 HTTP/1.1\r\nHost: x\r\n\r\n"
            )
            .unwrap();
        });
        let words = znimok_google::Words {
            done_title: "ok".into(),
            done_text: "ok".into(),
            failed_title: "no".into(),
        };
        let code = p.wait(std::time::Duration::from_secs(5), &words).unwrap();
        h.join().unwrap();
        let s = exchange(&f, "123.456", &code).unwrap();
        assert_eq!(
            s,
            Signed {
                team: "Plum Co".into(),
                keep: "oauth:xoxe-1-r1".into()
            }
        );
        let body = f.seen()[0].body_text();
        assert!(body.contains("code_verifier=") && !body.contains("client_secret"));
        // A refresh turns the refresh token over.
        let f = Fake::new(&[(
            200,
            r#"{"ok":true,"access_token":"xoxe.xoxp-2","refresh_token":"xoxe-1-r2","token_type":"user"}"#,
        )]);
        assert_eq!(
            refresh(&f, "123.456", "xoxe-1-r1").unwrap(),
            ("xoxe.xoxp-2".into(), "xoxe-1-r2".into())
        );
        let f = Fake::new(&[(200, r#"{"ok":false,"error":"invalid_refresh_token"}"#)]);
        let e = refresh(&f, "123.456", "x").unwrap_err();
        assert!(
            !e.retry() && e.to_string().contains("sign in to Slack again"),
            "{e}"
        );
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
