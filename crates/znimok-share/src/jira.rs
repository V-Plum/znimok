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
            base_url(&cfg.site)
        ),
        &auth(cfg, token),
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
    let base = base_url(&cfg.site);
    let a = auth(cfg, token);
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
    let base = base_url(&cfg.site);
    let a = auth(cfg, token);
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
