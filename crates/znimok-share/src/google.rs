//! Google Drive (ZK-260) through the account signed in in the settings (ZK-259, `drive.file`
//! only): the file goes into the «Znimok» folder of the person's Drive, back comes its link. The
//! upload is resumable (a session, then the bytes in one PUT), so a large recording is not
//! limited to the 5 MB of a simple upload; a failed one is started again by the queue.

use serde_json::{Value, json};
use znimok_models::http::{Response, Transport};

use crate::multipart::percent;
use crate::{Item, MAX_ANSWER, Sent, ShareError, status_error, timeout_for};

const DRIVE: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
/// The folder in Drive that what Znimok sends goes to.
pub const FOLDER: &str = "Znimok";

/// The OAuth client of the build: the Google Cloud project's «Desktop app» client, given at
/// build time (`ZNIMOK_GOOGLE_CLIENT_ID`, `ZNIMOK_GOOGLE_CLIENT_SECRET`; for an installed app
/// neither is a secret — PKCE protects the sign-in — but they stay out of git). A build without
/// them has no Google.
pub fn client() -> Option<znimok_google::Client> {
    let id = option_env!("ZNIMOK_GOOGLE_CLIENT_ID")?.trim();
    let secret = option_env!("ZNIMOK_GOOGLE_CLIENT_SECRET")
        .unwrap_or("")
        .trim();
    (!id.is_empty()).then(|| znimok_google::Client {
        id: id.to_string(),
        secret: secret.to_string(),
    })
}

/// The name of an account's refresh token in the OS store.
pub fn secret_name(account_id: &str) -> String {
    format!("share-google-{account_id}")
}

/// The token requests of [`znimok_google`] through the OS HTTP stack.
pub struct Form<'a>(pub &'a dyn Transport);

impl znimok_google::Post for Form<'_> {
    fn post_form(&self, url: &str, body: &str) -> Result<(u16, Vec<u8>), String> {
        self.0
            .request(
                "POST",
                url,
                &[],
                Some((body.as_bytes(), "application/x-www-form-urlencoded")),
                timeout_for(0),
                MAX_ANSWER,
            )
            .map(|r| (r.status, r.body))
            .map_err(|e| e.to_string())
    }
}

/// A fresh access token for the account's refresh token. A refused one (signed out at Google,
/// the password changed, half a year unused) stops: the person signs in again.
pub fn access(
    t: &dyn Transport,
    client: &znimok_google::Client,
    refresh_token: &str,
) -> Result<String, ShareError> {
    match znimok_google::refresh(&Form(t), client, refresh_token) {
        Ok(tokens) => Ok(tokens.access_token),
        Err(e) if e.starts_with("Google: HTTP 4") && !e.starts_with("Google: HTTP 429") => Err(
            ShareError::Fail(format!("Google: sign in again in the settings ({e})")),
        ),
        Err(e) => Err(ShareError::Again(e)),
    }
}

fn answer(r: &Response) -> Result<Value, ShareError> {
    if r.status == 403 && String::from_utf8_lossy(&r.body).contains("SCOPE_INSUFFICIENT") {
        return Err(ShareError::Fail(
            "Google Drive is not allowed for this account: sign in again and tick Google Drive"
                .into(),
        ));
    }
    if !(200..300).contains(&r.status) {
        return Err(status_error("Google Drive", r));
    }
    Ok(serde_json::from_slice(&r.body).unwrap_or(Value::Null))
}

fn get(t: &dyn Transport, auth: &str, url: &str) -> Result<Value, ShareError> {
    let r = t.request(
        "GET",
        url,
        &[("Authorization", auth)],
        None,
        timeout_for(0),
        MAX_ANSWER,
    )?;
    answer(&r)
}

fn post_json(t: &dyn Transport, auth: &str, url: &str, body: &Value) -> Result<Value, ShareError> {
    let b = body.to_string();
    let r = t.request(
        "POST",
        url,
        &[("Authorization", auth)],
        Some((b.as_bytes(), "application/json; charset=UTF-8")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    answer(&r)
}

/// The account behind the token and the folder: «v.v.plum@gmail.com → Znimok».
pub fn check(t: &dyn Transport, access_token: &str) -> Result<String, ShareError> {
    let auth = format!("Bearer {access_token}");
    let v = get(
        t,
        &auth,
        &format!("{DRIVE}/about?fields=user(emailAddress)"),
    )?;
    Ok(format!(
        "{} → {FOLDER}",
        v["user"]["emailAddress"].as_str().unwrap_or("?")
    ))
}

/// The «Znimok» folder: the one Znimok made before (with `drive.file` only those are seen), or a
/// new one.
fn folder(t: &dyn Transport, auth: &str) -> Result<String, ShareError> {
    let q = format!("name = '{FOLDER}' and mimeType = '{FOLDER_MIME}' and trashed = false");
    let v = get(
        t,
        auth,
        &format!(
            "{DRIVE}/files?q={}&spaces=drive&fields=files(id)&pageSize=1",
            percent(&q)
        ),
    )?;
    if let Some(id) = v["files"][0]["id"].as_str() {
        return Ok(id.to_string());
    }
    let v = post_json(
        t,
        auth,
        &format!("{DRIVE}/files?fields=id"),
        &json!({"name": FOLDER, "mimeType": FOLDER_MIME}),
    )?;
    v["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ShareError::Fail("Google Drive: no folder id".into()))
}

/// Uploads `bytes` into the «Znimok» folder; `link_anyone`: anyone with the link can view it.
pub fn send(
    t: &dyn Transport,
    access_token: &str,
    link_anyone: bool,
    item: &Item,
    bytes: &[u8],
) -> Result<Sent, ShareError> {
    let auth = format!("Bearer {access_token}");
    let parent = folder(t, &auth)?;
    // 1. The session: the file's name, place and words; Google answers with where to put it.
    let mut meta = json!({"name": item.file_name, "parents": [parent]});
    let words = [item.title.trim(), item.text.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !words.is_empty() {
        meta["description"] = Value::String(words);
    }
    let m = meta.to_string();
    let len = bytes.len().to_string();
    let r = t.request(
        "POST",
        &format!("{UPLOAD}?uploadType=resumable&fields=id,webViewLink"),
        &[
            ("Authorization", &auth),
            ("X-Upload-Content-Type", &item.mime),
            ("X-Upload-Content-Length", &len),
        ],
        Some((m.as_bytes(), "application/json; charset=UTF-8")),
        timeout_for(0),
        MAX_ANSWER,
    )?;
    answer(&r)?;
    let session = r
        .location
        .ok_or_else(|| ShareError::Again("Google Drive: no upload address".into()))?;
    // 2. The bytes, in one go.
    let r = t.request(
        "PUT",
        &session,
        &[],
        Some((bytes, &item.mime)),
        timeout_for(bytes.len()),
        MAX_ANSWER,
    )?;
    let v = answer(&r)?;
    let id = v["id"]
        .as_str()
        .ok_or_else(|| ShareError::Fail("Google Drive: no file id".into()))?
        .to_string();
    // 3. Who may open the link.
    if link_anyone {
        post_json(
            t,
            &auth,
            &format!("{DRIVE}/files/{id}/permissions?fields=id"),
            &json!({"role": "reader", "type": "anyone"}),
        )?;
    }
    Ok(Sent {
        url: Some(
            v["webViewLink"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("https://drive.google.com/file/d/{id}/view")),
        ),
    })
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
            text: "кнопка не та".into(),
            kind: "screenshot".into(),
            place: String::new(),
        }
    }

    #[test]
    fn a_new_folder_a_session_the_bytes_and_the_link() {
        let f = Fake::new(&[
            (200, r#"{"files":[]}"#),
            (200, r#"{"id":"FOLDER1"}"#),
            (200, "Location: https://upload.example/session1\n"),
            (
                200,
                r#"{"id":"F1","webViewLink":"https://drive.google.com/file/d/F1/view"}"#,
            ),
            (200, r#"{"id":"anyoneWithLink"}"#),
        ]);
        let s = send(&f, "at", true, &item(), b"PNG").unwrap();
        assert_eq!(
            s.url.as_deref(),
            Some("https://drive.google.com/file/d/F1/view")
        );
        let seen = f.seen();
        assert!(seen[0].url.contains("files?q=name%20%3D%20%27Znimok%27"));
        assert_eq!(seen[0].header("Authorization"), Some("Bearer at"));
        let v: Value = serde_json::from_slice(&seen[1].body).unwrap();
        assert_eq!(
            (v["name"].as_str(), v["mimeType"].as_str()),
            (Some("Znimok"), Some(FOLDER_MIME))
        );
        assert!(seen[2].url.contains("uploadType=resumable"));
        assert_eq!(seen[2].header("X-Upload-Content-Length"), Some("3"));
        let v: Value = serde_json::from_slice(&seen[2].body).unwrap();
        assert_eq!(v["parents"][0], "FOLDER1");
        assert_eq!(v["description"], "Знімок\nкнопка не та");
        assert_eq!(
            (seen[3].method.as_str(), seen[3].url.as_str()),
            ("PUT", "https://upload.example/session1")
        );
        assert_eq!(seen[3].body, b"PNG");
        assert_eq!(seen[3].header("Authorization"), None);
        assert!(seen[4].url.ends_with("/files/F1/permissions?fields=id"));
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn the_folder_made_before_and_no_link_for_all() {
        let f = Fake::new(&[
            (200, r#"{"files":[{"id":"OLD"}]}"#),
            (200, "Location: https://upload.example/s\n"),
            (201, r#"{"id":"F2"}"#),
        ]);
        let s = send(&f, "at", false, &item(), b"x").unwrap();
        assert_eq!(
            s.url.as_deref(),
            Some("https://drive.google.com/file/d/F2/view")
        );
        let seen = f.seen();
        assert_eq!(seen.len(), 3);
        let v: Value = serde_json::from_slice(&seen[1].body).unwrap();
        assert_eq!(v["parents"][0], "OLD");
    }

    #[test]
    fn drive_left_unticked_and_a_busy_drive() {
        let f = Fake::new(&[(
            403,
            r#"{"error":{"code":403,"details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#,
        )]);
        let e = send(&f, "at", false, &item(), b"x").unwrap_err();
        assert!(
            !e.retry() && e.to_string().contains("tick Google Drive"),
            "{e}"
        );
        let f = Fake::new(&[(503, "busy")]);
        assert!(send(&f, "at", false, &item(), b"x").unwrap_err().retry());
        let f = Fake::new(&[(200, r#"{"user":{"emailAddress":"a@b.c"}}"#)]);
        assert_eq!(check(&f, "at").unwrap(), "a@b.c → Znimok");
    }

    #[test]
    fn a_refused_refresh_token_asks_to_sign_in_again() {
        let c = znimok_google::Client {
            id: "id".into(),
            secret: "s".into(),
        };
        let f = Fake::new(&[(
            400,
            r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
        )]);
        let e = access(&f, &c, "rt").unwrap_err();
        assert!(!e.retry() && e.to_string().contains("sign in again"), "{e}");
        let f = Fake::new(&[(200, r#"{"access_token":"at2","expires_in":3599}"#)]);
        assert_eq!(access(&f, &c, "rt").unwrap(), "at2");
        assert!(f.seen()[0].body_text().contains("grant_type=refresh_token"));
        let f = Fake::new(&[(503, "busy")]);
        assert!(access(&f, &c, "rt").unwrap_err().retry());
    }
}
