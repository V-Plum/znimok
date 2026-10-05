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
/// The conversations too (ZK-289): one-to-one and group messages, and the people's names.
pub const USER_SCOPES: &str =
    "files:write,chat:write,channels:read,groups:read,im:read,mpim:read,users:read";
/// What a stored token starts with when it is a sign-in's refresh token (else a pasted one).
pub const SIGNED_IN: &str = "oauth:";

/// The client id of Znimok's Slack app (PKCE, a public client: no secret), given at build time
/// (`ZNIMOK_SLACK_CLIENT_ID`); a build without it has no «Sign in to Slack».
pub fn client_id() -> Option<&'static str> {
    option_env!("ZNIMOK_SLACK_CLIENT_ID")
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The app's secret (`ZNIMOK_SLACK_CLIENT_SECRET`, ZK-288): a sign-in through the site's https
/// page is a web sign-in to Slack, and the refresh of its turned-over token wants the secret —
/// only a desktop sign-in (localhost) refreshes without it.
pub fn client_secret() -> Option<&'static str> {
    option_env!("ZNIMOK_SLACK_CLIENT_SECRET")
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
    secret: Option<&str>,
    refresh_token: &str,
) -> Result<(String, String), ShareError> {
    let mut pairs = vec![
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    if let Some(s) = secret {
        pairs.push(("client_secret", s));
    }
    let v = form_call(t, &pairs).map_err(|e| match e {
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

/// One paged Slack list (`conversations.list`, `users.list`): the items under `key`.
fn paged(t: &dyn Transport, a: &str, url: &str, key: &str) -> Result<Vec<Value>, ShareError> {
    let mut out = Vec::new();
    let mut cursor = String::new();
    for _ in 0..10 {
        let mut u = url.to_string();
        if !cursor.is_empty() {
            u.push_str(&format!("&cursor={}", percent(&cursor)));
        }
        let r = t.request(
            "GET",
            &u,
            &[("Authorization", a)],
            None,
            timeout_for(0),
            MAX_ANSWER,
        )?;
        let v = answer(&r)?;
        out.extend(v[key].as_array().into_iter().flatten().cloned());
        cursor = v["response_metadata"]["next_cursor"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if cursor.is_empty() {
            break;
        }
    }
    Ok(out)
}

fn missing_scope(e: &ShareError) -> bool {
    matches!(e, ShareError::Fail(m) if m.contains("missing_scope"))
}

/// `mpdm-anna--bohdan--plum-1` → `anna, bohdan, plum`.
fn group_name(raw: &str) -> String {
    let s = raw.trim_start_matches("mpdm-");
    let s = s
        .rsplit_once('-')
        .filter(|(_, n)| n.chars().all(|c| c.is_ascii_digit()))
        .map_or(s, |(a, _)| a);
    s.split("--").collect::<Vec<_>>().join(", ")
}

/// Where the person can post (ZK-278, ZK-289): the channels (public, and private ones they are
/// in) by name, then the one-to-one and group conversations with the people's names. An older
/// sign-in without im:read / mpim:read / users:read still lists the channels.
pub fn channels(t: &dyn Transport, token: &str) -> Result<Vec<(String, String)>, ShareError> {
    let a = bearer(token);
    let list = |types: &str| {
        paged(
            t,
            &a,
            &format!("{API}/conversations.list?types={types}&exclude_archived=true&limit=200"),
            "channels",
        )
    };
    let chans = list("public_channel,private_channel").map_err(|e| {
        if missing_scope(&e) {
            ShareError::Fail(
                "Slack: the app may not list channels — make it again with «Create the Slack app» or type the channel's ID".into(),
            )
        } else {
            e
        }
    })?;
    let mut out: Vec<(String, String)> = chans
        .iter()
        .filter_map(|c| {
            Some((
                c["id"].as_str()?.to_string(),
                format!("#{}", c["name"].as_str()?),
            ))
        })
        .collect();
    out.sort_by_key(|p| p.1.to_lowercase());
    // The conversations are extra: without them (an older sign-in, a hiccup) the channels stay.
    let Ok(talks) = list("im,mpim") else {
        return Ok(out);
    };
    // The people's names for the one-to-one ones; without users:read their ids.
    let names: std::collections::HashMap<String, String> =
        if talks.iter().any(|c| c["is_im"] == true) {
            match paged(t, &a, &format!("{API}/users.list?limit=200"), "members") {
                Ok(users) => users
                    .iter()
                    .filter_map(|u| {
                        let id = u["id"].as_str()?.to_string();
                        let p = &u["profile"];
                        let name = [&p["display_name"], &p["real_name"], &u["name"]]
                            .into_iter()
                            .filter_map(|v| v.as_str())
                            .find(|s| !s.trim().is_empty())?
                            .to_string();
                        Some((id, name))
                    })
                    .collect(),
                Err(_) => Default::default(),
            }
        } else {
            Default::default()
        };
    let mut more: Vec<(String, String)> = talks
        .iter()
        .filter(|c| c["is_user_deleted"] != true)
        .filter_map(|c| {
            let id = c["id"].as_str()?.to_string();
            let name = if c["is_im"] == true {
                let user = c["user"].as_str().unwrap_or("?");
                format!("@{}", names.get(user).map_or(user, String::as_str))
            } else {
                group_name(c["name"].as_str().unwrap_or(""))
            };
            Some((id, name))
        })
        .collect();
    more.sort_by_key(|p| p.1.to_lowercase());
    out.extend(more);
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
    // As a form, the way Slack's own SDKs call it (ZK-293): with a JSON body the file was uploaded
    // and the answer «ok», but it was shared nowhere.
    let files = json!([{"id": file_id, "title": item.title}]).to_string();
    let body = znimok_google::form_body(&[
        ("files", files.as_str()),
        ("channel_id", channel.trim()),
        ("initial_comment", comment.as_str()),
    ]);
    let r = t.request(
        "POST",
        &format!("{API}/files.completeUploadExternal"),
        &[("Authorization", &a)],
        Some((body.as_bytes(), "application/x-www-form-urlencoded")),
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
        // A form (ZK-293): channel_id and files (a JSON string) as fields.
        assert_eq!(seen[2].content_type, "application/x-www-form-urlencoded");
        let b = seen[2].body_text();
        assert!(b.contains("channel_id=C123"), "{b}");
        assert!(b.contains("files=%5B%7B%22id%22%3A%22F1%22"), "{b}");
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
    fn conversations_after_the_channels_with_names() {
        let f = Fake::new(&[
            (
                200,
                r#"{"ok":true,"channels":[{"id":"C1","name":"general"}]}"#,
            ),
            (
                200,
                r#"{"ok":true,"channels":[{"id":"D1","is_im":true,"user":"U2"},{"id":"G1","is_mpim":true,"name":"mpdm-anna--plum-1"},{"id":"D9","is_im":true,"user":"U9","is_user_deleted":true}]}"#,
            ),
            (
                200,
                r#"{"ok":true,"members":[{"id":"U2","name":"b","profile":{"display_name":"","real_name":"Bohdan"}}]}"#,
            ),
        ]);
        let c = channels(&f, "xoxp").unwrap();
        assert_eq!(
            c,
            [
                ("C1".to_string(), "#general".to_string()),
                ("D1".into(), "@Bohdan".into()),
                ("G1".into(), "anna, plum".into()),
            ]
        );
        assert!(f.seen()[1].url.contains("types=im,mpim"));
        // An older sign-in without the scopes: the channels all the same.
        let f = Fake::new(&[
            (
                200,
                r#"{"ok":true,"channels":[{"id":"C1","name":"general"}]}"#,
            ),
            (200, r#"{"ok":false,"error":"missing_scope"}"#),
        ]);
        assert_eq!(channels(&f, "xoxp").unwrap().len(), 1);
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
            refresh(&f, "123.456", None, "xoxe-1-r1").unwrap(),
            ("xoxe.xoxp-2".into(), "xoxe-1-r2".into())
        );
        let f = Fake::new(&[(200, r#"{"ok":false,"error":"invalid_refresh_token"}"#)]);
        let e = refresh(&f, "123.456", None, "x").unwrap_err();
        // A web sign-in's refresh carries the secret (ZK-288).
        let f = Fake::new(&[(200, r#"{"ok":true,"access_token":"a","refresh_token":"r"}"#)]);
        refresh(&f, "123.456", Some("sec"), "x").unwrap();
        assert!(f.seen()[0].body_text().contains("client_secret=sec"));
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
