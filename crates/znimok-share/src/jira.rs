//! Jira Cloud (REST v3) with the user's e-mail and an API token
//! (id.atlassian.com → Security → API tokens): the file attached to an issue — the one in the
//! settings, or a new one in the project each time — with the words as a comment or as the new
//! issue's description.

use base64::Engine;
use serde_json::{Value, json};
use znimok_models::http::Transport;
use znimok_settings::JiraTarget;

use crate::multipart::Form;
use crate::{Item, MAX_ANSWER, Sent, ShareError, status_error, timeout_for};

/// `x.atlassian.net`, `https://x.atlassian.net/` → `https://x.atlassian.net`.
pub fn base_url(site: &str) -> String {
    let s = site.trim().trim_end_matches('/');
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("https://{s}")
    }
}

/// What `token` is when the account signed in through Atlassian (ZK-273): `bearer:<cloud id>:
/// <access token>` — the calls then go to api.atlassian.com, not to the site.
pub const SIGNED_IN_CALL: &str = "bearer:";

/// Where the calls go and with what: the site with the e-mail and an API token, or Atlassian's
/// API for a signed-in account.
fn conn(cfg: &JiraTarget, token: &str) -> (String, String) {
    if let Some((cloud, access)) = token
        .strip_prefix(SIGNED_IN_CALL)
        .and_then(|r| r.split_once(':'))
    {
        return (
            format!("https://api.atlassian.com/ex/jira/{cloud}"),
            format!("Bearer {access}"),
        );
    }
    (base_url(&cfg.site), auth(cfg, token))
}

// ------------------------------------------------------------------ signing in (ZK-273)

const AUTH_URL: &str = "https://auth.atlassian.com/authorize";
const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";
/// Reading and writing issues, who the person is, and staying signed in.
pub const SCOPES: &str = "read:jira-work write:jira-work read:jira-user offline_access";

/// Znimok's Atlassian app (OAuth 2.0 3LO), given at build time (`ZNIMOK_ATLASSIAN_CLIENT_ID`,
/// `ZNIMOK_ATLASSIAN_CLIENT_SECRET` — Atlassian wants the secret even from a desktop app); a
/// build without it has no «Sign in to Atlassian».
pub fn client() -> Option<znimok_google::Client> {
    let id = option_env!("ZNIMOK_ATLASSIAN_CLIENT_ID")?.trim();
    let secret = option_env!("ZNIMOK_ATLASSIAN_CLIENT_SECRET")
        .unwrap_or("")
        .trim();
    (!id.is_empty() && !secret.is_empty()).then(|| znimok_google::Client {
        id: id.to_string(),
        secret: secret.to_string(),
    })
}

/// Starts «Sign in to Atlassian» on Znimok's registered address (Slack's, ZK-273).
pub fn sign_in(client: &znimok_google::Client) -> std::io::Result<znimok_google::Pending> {
    znimok_google::begin(
        AUTH_URL,
        &[
            ("audience", "api.atlassian.com"),
            ("client_id", &client.id),
            ("scope", SCOPES),
            ("prompt", "consent"),
        ],
        Some(crate::slack::SIGN_IN_PORT),
        crate::slack::SIGN_IN_PATH,
    )
}

/// A site the signed-in person may reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub cloud_id: String,
    /// `https://team.atlassian.net`
    pub url: String,
    pub name: String,
}

/// A sign-in's result: the sites, and what to keep (the refresh token, marked as Slack's are).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    pub sites: Vec<Site>,
    pub keep: String,
}

fn token_call(t: &dyn Transport, body: &Value) -> Result<(String, Option<String>), ShareError> {
    let b = body.to_string();
    let r = t.request(
        "POST",
        TOKEN_URL,
        &[("Accept", "application/json")],
        Some((b.as_bytes(), "application/json")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    if r.status != 200 {
        let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
        let why = v["error_description"]
            .as_str()
            .or(v["error"].as_str())
            .unwrap_or("")
            .to_string();
        return Err(if r.status >= 500 || r.status == 429 {
            ShareError::Again(format!("Atlassian: HTTP {} {why}", r.status))
        } else {
            ShareError::Fail(format!("Atlassian: {why} — sign in again"))
        });
    }
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    let access = v["access_token"]
        .as_str()
        .ok_or_else(|| ShareError::Fail("Atlassian gave no token".into()))?;
    Ok((
        access.to_string(),
        v["refresh_token"].as_str().map(str::to_string),
    ))
}

/// The code for tokens, and the sites they reach.
pub fn exchange(
    t: &dyn Transport,
    client: &znimok_google::Client,
    code: &znimok_google::Code,
) -> Result<Signed, ShareError> {
    let (access, refresh) = token_call(
        t,
        &json!({
            "grant_type": "authorization_code",
            "client_id": client.id,
            "client_secret": client.secret,
            "code": code.code,
            "redirect_uri": code.redirect(),
            "code_verifier": code.verifier(),
        }),
    )?;
    let refresh =
        refresh.ok_or_else(|| ShareError::Fail("Atlassian gave no refresh token".into()))?;
    let v = json_call(t, "GET", RESOURCES_URL, &format!("Bearer {access}"), None)?;
    let sites = v
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| {
            s["scopes"].as_array().is_none_or(|a| {
                a.iter()
                    .any(|x| x.as_str().is_some_and(|x| x.contains("jira")))
            })
        })
        .filter_map(|s| {
            Some(Site {
                cloud_id: s["id"].as_str()?.to_string(),
                url: s["url"].as_str().unwrap_or("").to_string(),
                name: s["name"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect::<Vec<_>>();
    if sites.is_empty() {
        return Err(ShareError::Fail(
            "Atlassian: this account has no Jira site".into(),
        ));
    }
    Ok(Signed {
        sites,
        keep: format!("{}{refresh}", crate::slack::SIGNED_IN),
    })
}

/// The site a sign-in goes to: the one typed in the settings, else the first.
pub fn pick<'a>(sites: &'a [Site], typed: &str) -> Option<&'a Site> {
    let host = |u: &str| {
        u.trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_lowercase()
    };
    let want = host(typed);
    sites
        .iter()
        .find(|s| !want.is_empty() && host(&s.url) == want)
        .or_else(|| sites.first())
}

/// A fresh access token, and the refresh token to keep (Atlassian turns it over).
pub fn refresh(
    t: &dyn Transport,
    client: &znimok_google::Client,
    refresh_token: &str,
) -> Result<(String, String), ShareError> {
    let (access, next) = token_call(
        t,
        &json!({
            "grant_type": "refresh_token",
            "client_id": client.id,
            "client_secret": client.secret,
            "refresh_token": refresh_token,
        }),
    )?;
    Ok((access, next.unwrap_or_else(|| refresh_token.to_string())))
}

fn auth(cfg: &JiraTarget, token: &str) -> String {
    let raw = format!("{}:{}", cfg.email.trim(), token);
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw)
    )
}

fn json_call(
    t: &dyn Transport,
    method: &str,
    url: &str,
    auth: &str,
    body: Option<&Value>,
) -> Result<Value, ShareError> {
    let text = body.map(|b| b.to_string());
    let r = t.request(
        method,
        url,
        &[("Authorization", auth), ("Accept", "application/json")],
        text.as_deref().map(|b| (b.as_bytes(), "application/json")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    if (200..300).contains(&r.status) {
        return Ok(serde_json::from_slice(&r.body).unwrap_or(Value::Null));
    }
    Err(match r.status {
        401 => ShareError::Fail("Jira: the e-mail or the API token is not accepted".into()),
        403 => ShareError::Fail("Jira: no permission for this".into()),
        _ => status_error("Jira", &r),
    })
}

/// A paragraph of plain text in the Atlassian Document Format.
fn adf(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                json!({"type": "paragraph"})
            } else {
                json!({"type": "paragraph", "content": [{"type": "text", "text": line}]})
            }
        })
        .collect();
    json!({"type": "doc", "version": 1, "content": paragraphs})
}

/// The projects the person may see (ZK-278), the ones worked on lately first: `(KEY, KEY · Name)`.
pub fn projects(
    t: &dyn Transport,
    cfg: &JiraTarget,
    token: &str,
) -> Result<Vec<(String, String)>, ShareError> {
    let v = json_call(
        t,
        "GET",
        &format!(
            "{}/rest/api/3/project/search?maxResults=100&orderBy=-lastIssueUpdatedTime",
            conn(cfg, token).0
        ),
        &conn(cfg, token).1,
        None,
    )?;
    Ok(v["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let key = p["key"].as_str()?;
            Some((
                key.to_string(),
                format!("{key} · {}", p["name"].as_str().unwrap_or("")),
            ))
        })
        .collect())
}

/// Who the token is, and that the project (and the issue) are there.
pub fn check(t: &dyn Transport, cfg: &JiraTarget, token: &str) -> Result<String, ShareError> {
    let (base, a) = conn(cfg, token);
    let me = json_call(t, "GET", &format!("{base}/rest/api/3/myself"), &a, None)?;
    let who = me["displayName"].as_str().unwrap_or("?").to_string();
    let mut out = who;
    if !cfg.project.trim().is_empty() {
        let p = json_call(
            t,
            "GET",
            &format!("{base}/rest/api/3/project/{}", cfg.project.trim()),
            &a,
            None,
        )?;
        out = format!(
            "{out} · {} ({})",
            p["name"].as_str().unwrap_or(""),
            cfg.project.trim()
        );
    }
    if !cfg.issue.trim().is_empty() {
        let i = json_call(
            t,
            "GET",
            &format!(
                "{base}/rest/api/3/issue/{}?fields=summary",
                cfg.issue.trim()
            ),
            &a,
            None,
        )?;
        out = format!("{out} · {}", i["key"].as_str().unwrap_or(cfg.issue.trim()));
    }
    Ok(out)
}

pub fn send(
    t: &dyn Transport,
    cfg: &JiraTarget,
    token: &str,
    item: &Item,
    bytes: &[u8],
) -> Result<Sent, ShareError> {
    let (base, a) = conn(cfg, token);
    // The issue: the one in the settings, or a new one.
    let key = if cfg.issue.trim().is_empty() {
        if cfg.project.trim().is_empty() {
            return Err(ShareError::Fail("Jira: no project in the settings".into()));
        }
        let mut fields = json!({
            "project": {"key": cfg.project.trim()},
            "summary": item.title.chars().take(250).collect::<String>(),
            "issuetype": {"name": if cfg.issue_type.trim().is_empty() { "Task" } else { cfg.issue_type.trim() }},
        });
        if !item.text.trim().is_empty() {
            fields["description"] = adf(item.text.trim());
        }
        let created = json_call(
            t,
            "POST",
            &format!("{base}/rest/api/3/issue"),
            &a,
            Some(&json!({"fields": fields})),
        )?;
        created["key"]
            .as_str()
            .ok_or_else(|| ShareError::Fail("Jira: the new issue has no key".into()))?
            .to_string()
    } else {
        cfg.issue.trim().to_string()
    };
    // The file.
    let (body, ct) = Form::new()
        .file("file", &item.file_name, &item.mime, bytes)
        .finish();
    let r = t.request(
        "POST",
        &format!("{base}/rest/api/3/issue/{key}/attachments"),
        &[
            ("Authorization", &a),
            ("X-Atlassian-Token", "no-check"),
            ("Accept", "application/json"),
        ],
        Some((&body, &ct)),
        timeout_for(bytes.len()),
        MAX_ANSWER,
    )?;
    if !(200..300).contains(&r.status) {
        return Err(match r.status {
            413 => ShareError::Fail("Jira: the file is larger than the site allows".into()),
            _ => status_error("Jira", &r),
        });
    }
    // The words as a comment on an existing issue (a new one has them as its description).
    if !cfg.issue.trim().is_empty() && !item.text.trim().is_empty() {
        let comment = format!("{}\n{}", item.title, item.text.trim());
        json_call(
            t,
            "POST",
            &format!("{base}/rest/api/3/issue/{key}/comment"),
            &a,
            Some(&json!({"body": adf(&comment)})),
        )?;
    }
    Ok(Sent {
        url: Some(format!("{base}/browse/{key}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Fake;

    fn cfg(issue: &str) -> JiraTarget {
        JiraTarget {
            enabled: true,
            site: "plum.atlassian.net".into(),
            email: "a@b.c".into(),
            project: "ZT".into(),
            issue: issue.into(),
            issue_type: "Task".into(),
            ..Default::default()
        }
    }

    fn item() -> Item {
        Item {
            file_name: "bug.zreport".into(),
            mime: "application/zip".into(),
            title: "Кнопка не працює".into(),
            text: "кроки:\n1. натиснути".into(),
            kind: "report".into(),
            place: String::new(),
        }
    }

    #[test]
    fn a_new_issue_then_the_file() {
        let f = Fake::new(&[(201, r#"{"key":"ZT-7"}"#), (200, "[]")]);
        let s = send(&f, &cfg(""), "tok", &item(), b"PK").unwrap();
        assert_eq!(
            s.url.as_deref(),
            Some("https://plum.atlassian.net/browse/ZT-7")
        );
        let seen = f.seen();
        assert_eq!(seen[0].url, "https://plum.atlassian.net/rest/api/3/issue");
        let v: Value = serde_json::from_slice(&seen[0].body).unwrap();
        assert_eq!(v["fields"]["project"]["key"], "ZT");
        assert_eq!(v["fields"]["summary"], "Кнопка не працює");
        assert_eq!(
            v["fields"]["description"]["content"][1]["content"][0]["text"],
            "1. натиснути"
        );
        assert!(
            seen[0]
                .header("Authorization")
                .unwrap()
                .starts_with("Basic ")
        );
        assert_eq!(
            seen[1].url,
            "https://plum.atlassian.net/rest/api/3/issue/ZT-7/attachments"
        );
        assert_eq!(seen[1].header("X-Atlassian-Token"), Some("no-check"));
        assert!(seen[1].body_text().contains("filename=\"bug.zreport\""));
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn an_existing_issue_gets_the_file_and_a_comment() {
        let f = Fake::new(&[(200, "[]"), (201, "{}")]);
        send(&f, &cfg("ZT-1"), "tok", &item(), b"PK").unwrap();
        let seen = f.seen();
        assert_eq!(
            seen[0].url,
            "https://plum.atlassian.net/rest/api/3/issue/ZT-1/attachments"
        );
        assert_eq!(
            seen[1].url,
            "https://plum.atlassian.net/rest/api/3/issue/ZT-1/comment"
        );
    }

    #[test]
    fn a_wrong_token_says_so() {
        let f = Fake::new(&[(401, "")]);
        let e = check(&f, &cfg(""), "tok").unwrap_err();
        assert!(!e.retry() && e.to_string().contains("not accepted"));
        assert_eq!(
            base_url("https://x.atlassian.net/"),
            "https://x.atlassian.net"
        );
    }

    #[test]
    fn signed_in_calls_go_through_atlassian() {
        let cfg = JiraTarget {
            site: "plum.atlassian.net".into(),
            email: "a@b.c".into(),
            ..Default::default()
        };
        let (base, a) = conn(&cfg, "tok");
        assert_eq!(base, "https://plum.atlassian.net");
        assert!(a.starts_with("Basic "));
        let (base, a) = conn(&cfg, "bearer:c-1:at");
        assert_eq!(
            (base.as_str(), a.as_str()),
            ("https://api.atlassian.com/ex/jira/c-1", "Bearer at")
        );
        let sites = [
            Site {
                cloud_id: "1".into(),
                url: "https://a.atlassian.net".into(),
                name: "A".into(),
            },
            Site {
                cloud_id: "2".into(),
                url: "https://b.atlassian.net".into(),
                name: "B".into(),
            },
        ];
        assert_eq!(pick(&sites, "b.atlassian.net").unwrap().cloud_id, "2");
        assert_eq!(pick(&sites, "").unwrap().cloud_id, "1");
        // The refresh turns the token over; a refused one asks to sign in again.
        let c = znimok_google::Client {
            id: "i".into(),
            secret: "s".into(),
        };
        let f = Fake::new(&[(200, r#"{"access_token":"a2","refresh_token":"r2"}"#)]);
        assert_eq!(refresh(&f, &c, "r1").unwrap(), ("a2".into(), "r2".into()));
        let v: Value = serde_json::from_slice(&f.seen()[0].body).unwrap();
        assert_eq!(
            (v["grant_type"].as_str(), v["client_secret"].as_str()),
            (Some("refresh_token"), Some("s"))
        );
        let f = Fake::new(&[(
            403,
            r#"{"error":"unauthorized_client","error_description":"refresh_token is invalid"}"#,
        )]);
        let e = refresh(&f, &c, "r1").unwrap_err();
        assert!(!e.retry() && e.to_string().contains("sign in again"), "{e}");
    }

    #[test]
    fn the_projects_lately_worked_on_first() {
        let f = crate::fake::Fake::new(&[(
            200,
            r#"{"values":[{"key":"ZK","name":"Znimok"},{"key":"AH","name":"Home"}]}"#,
        )]);
        let cfg = JiraTarget {
            site: "x.atlassian.net".into(),
            email: "a@b.c".into(),
            ..Default::default()
        };
        let p = projects(&f, &cfg, "tok").unwrap();
        assert_eq!(p[0], ("ZK".to_string(), "ZK · Znimok".to_string()));
        assert_eq!(p.len(), 2);
        assert!(
            f.seen()[0]
                .url
                .starts_with("https://x.atlassian.net/rest/api/3/project/search?")
        );
    }
}
