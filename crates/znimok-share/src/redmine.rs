//! Redmine (REST API switched on in its administration) with the user's API key (My account →
//! API access key): the file uploaded, then attached to the issue in the settings with the words
//! as a note, or to a new issue in the project.

use serde_json::{Value, json};
use znimok_models::http::Transport;
use znimok_settings::RedmineTarget;

use crate::multipart::percent;
use crate::{Item, MAX_ANSWER, Sent, ShareError, status_error, timeout_for};

fn base(cfg: &RedmineTarget) -> String {
    cfg.url.trim().trim_end_matches('/').to_string()
}

fn fail(r: &znimok_models::http::Response) -> ShareError {
    match r.status {
        401 => ShareError::Fail("Redmine: the API key is not accepted".into()),
        403 => ShareError::Fail("Redmine: no permission for this".into()),
        404 => {
            ShareError::Fail("Redmine: not found — the address, the project or the issue".into())
        }
        422 => ShareError::Fail(format!(
            "Redmine: {}",
            String::from_utf8_lossy(&r.body)
                .chars()
                .take(300)
                .collect::<String>()
        )),
        _ => status_error("Redmine", r),
    }
}

/// The projects the key may see (ZK-278): `(identifier, name)`, by name.
pub fn projects(
    t: &dyn Transport,
    cfg: &RedmineTarget,
    key: &str,
) -> Result<Vec<(String, String)>, ShareError> {
    let r = t.request(
        "GET",
        &format!("{}/projects.json?limit=100", base(cfg)),
        &[("X-Redmine-API-Key", key), ("Accept", "application/json")],
        None,
        timeout_for(0),
        MAX_ANSWER,
    )?;
    if r.status != 200 {
        return Err(fail(&r));
    }
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    let mut out: Vec<(String, String)> = v["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p["identifier"].as_str()?.to_string(),
                p["name"].as_str().unwrap_or("").to_string(),
            ))
        })
        .collect();
    out.sort_by_key(|p| p.1.to_lowercase());
    Ok(out)
}

pub fn check(t: &dyn Transport, cfg: &RedmineTarget, key: &str) -> Result<String, ShareError> {
    let r = t.request(
        "GET",
        &format!("{}/users/current.json", base(cfg)),
        &[("X-Redmine-API-Key", key)],
        None,
        timeout_for(0),
        MAX_ANSWER,
    )?;
    if r.status != 200 {
        return Err(fail(&r));
    }
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    let mut out = v["user"]["login"].as_str().unwrap_or("?").to_string();
    if !cfg.issue.trim().is_empty() {
        out = format!("{out} → #{}", cfg.issue.trim());
    } else if !cfg.project.trim().is_empty() {
        out = format!("{out} → {}", cfg.project.trim());
    }
    Ok(out)
}

pub fn send(
    t: &dyn Transport,
    cfg: &RedmineTarget,
    key: &str,
    item: &Item,
    bytes: &[u8],
) -> Result<Sent, ShareError> {
    let b = base(cfg);
    let h = [("X-Redmine-API-Key", key), ("Accept", "application/json")];
    // 1. The upload gives a token.
    let r = t.request(
        "POST",
        &format!("{b}/uploads.json?filename={}", percent(&item.file_name)),
        &h,
        Some((bytes, "application/octet-stream")),
        timeout_for(bytes.len()),
        MAX_ANSWER,
    )?;
    if !(200..300).contains(&r.status) {
        return Err(match r.status {
            413 | 422 => {
                ShareError::Fail("Redmine: the file is larger than the server allows".into())
            }
            _ => fail(&r),
        });
    }
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    let token = v["upload"]["token"]
        .as_str()
        .ok_or_else(|| ShareError::Fail("Redmine: no upload token".into()))?;
    let upload = json!([{"token": token, "filename": item.file_name, "content_type": item.mime}]);
    // 2. Attached: a note on the issue, or a new issue.
    if !cfg.issue.trim().is_empty() {
        let id = cfg.issue.trim().trim_start_matches('#');
        let mut notes = item.title.clone();
        if !item.text.trim().is_empty() {
            notes = format!("{notes}\n{}", item.text.trim());
        }
        let body = json!({"issue": {"notes": notes, "uploads": upload}}).to_string();
        let r = t.request(
            "PUT",
            &format!("{b}/issues/{id}.json"),
            &h,
            Some((body.as_bytes(), "application/json")),
            timeout_for(0),
            MAX_ANSWER,
        )?;
        if !(200..300).contains(&r.status) {
            return Err(fail(&r));
        }
        return Ok(Sent {
            url: Some(format!("{b}/issues/{id}")),
        });
    }
    if cfg.project.trim().is_empty() {
        return Err(ShareError::Fail(
            "Redmine: no project or issue in the settings".into(),
        ));
    }
    let body = json!({"issue": {
        "project_id": cfg.project.trim(),
        "subject": item.title.chars().take(250).collect::<String>(),
        "description": item.text,
        "uploads": upload,
    }})
    .to_string();
    let r = t.request(
        "POST",
        &format!("{b}/issues.json"),
        &h,
        Some((body.as_bytes(), "application/json")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    if !(200..300).contains(&r.status) {
        return Err(fail(&r));
    }
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    Ok(Sent {
        url: v["issue"]["id"]
            .as_i64()
            .map(|id| format!("{b}/issues/{id}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Fake;

    fn cfg(issue: &str) -> RedmineTarget {
        RedmineTarget {
            enabled: true,
            url: "https://rm.example.com/".into(),
            project: "web".into(),
            issue: issue.into(),
            ..Default::default()
        }
    }

    #[test]
    fn upload_then_a_note() {
        let f = Fake::new(&[(201, r#"{"upload":{"token":"7.abc"}}"#), (204, "")]);
        let item = Item {
            file_name: "a.png".into(),
            mime: "image/png".into(),
            title: "Знімок".into(),
            ..Default::default()
        };
        let s = send(&f, &cfg("#42"), "k", &item, b"PNG").unwrap();
        assert_eq!(s.url.as_deref(), Some("https://rm.example.com/issues/42"));
        let seen = f.seen();
        assert_eq!(
            seen[0].url,
            "https://rm.example.com/uploads.json?filename=a.png"
        );
        assert_eq!(seen[0].content_type, "application/octet-stream");
        assert_eq!(seen[0].header("X-Redmine-API-Key"), Some("k"));
        assert_eq!(seen[1].method, "PUT");
        let v: Value = serde_json::from_slice(&seen[1].body).unwrap();
        assert_eq!(v["issue"]["uploads"][0]["token"], "7.abc");
    }

    #[test]
    fn upload_then_a_new_issue() {
        let f = Fake::new(&[
            (201, r#"{"upload":{"token":"t"}}"#),
            (201, r#"{"issue":{"id":9}}"#),
        ]);
        let s = send(&f, &cfg(""), "k", &Item::default(), b"x").unwrap();
        assert_eq!(s.url.as_deref(), Some("https://rm.example.com/issues/9"));
        let v: Value = serde_json::from_slice(&f.seen()[1].body).unwrap();
        assert_eq!(v["issue"]["project_id"], "web");
    }

    #[test]
    fn the_projects_by_name() {
        let f = crate::fake::Fake::new(&[(
            200,
            r#"{"projects":[{"identifier":"web","name":"Website"},{"identifier":"app","name":"App"}]}"#,
        )]);
        let cfg = RedmineTarget {
            url: "https://rm.example.com/".into(),
            ..Default::default()
        };
        let p = projects(&f, &cfg, "k").unwrap();
        assert_eq!(p[0], ("app".to_string(), "App".to_string()));
        assert_eq!(
            f.seen()[0].url,
            "https://rm.example.com/projects.json?limit=100"
        );
    }
}
