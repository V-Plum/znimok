//! Google for Znimok (ZK-259): signing in as a desktop app.
//!
//! OAuth 2.0 for installed apps, as Google describes it: the system browser opens Google's sign-in
//! with a PKCE challenge; Google sends the person back to `http://127.0.0.1:<port>/` where
//! [`Pending::wait`] answers with a short page and takes the code; the code and the verifier buy
//! a refresh token ([`exchange`]), which buys access tokens when they are needed ([`refresh`]).
//! Znimok never sees the password.
//!
//! Only `drive.file` — the files Znimok itself made — so no sensitive scope and no Google app
//! review (the owner's decision, 04.10.2026). The client id and its «secret» are compiled in: for
//! a desktop client Google does not treat the secret as one; PKCE is what protects the code.
//!
//! The HTTP goes through [`Post`], the caller's transport (the app's, with its proxy and limits).

use base64::Engine;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

/// Where the person signs in.
pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
/// Where codes and refresh tokens become access tokens.
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Where a refresh token is revoked on «Sign out».
pub const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
/// The files Znimok made, nothing else; and who the person is (to show the account).
pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive.file",
    "openid",
    "email",
];

/// The OAuth client of the Google Cloud project (type «Desktop app»).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
    pub id: String,
    pub secret: String,
}

/// A form POST (`application/x-www-form-urlencoded`): the status and the body.
pub trait Post {
    fn post_form(&self, url: &str, body: &str) -> Result<(u16, Vec<u8>), String>;
}

/// What the token endpoint gives.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    /// Only on the first exchange (and when Google rotates it).
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Seconds the access token lives.
    #[serde(default)]
    pub expires_in: u64,
    /// The OpenID token: who signed in.
    #[serde(default)]
    pub id_token: Option<String>,
}

impl Tokens {
    /// The account's e-mail from the OpenID token (its payload; the token came straight from
    /// Google over TLS, so its signature is not checked here).
    pub fn email(&self) -> Option<String> {
        let payload = self.id_token.as_ref()?.split('.').nth(1)?;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload.trim_end_matches('='))
            .ok()?;
        let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
        v["email"].as_str().map(str::to_string)
    }
}

fn b64url(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// A PKCE pair (RFC 7636): the verifier (kept) and its S256 challenge (sent).
pub fn pkce() -> (String, String) {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    let verifier = b64url(&bytes);
    let challenge = challenge_of(&verifier);
    (verifier, challenge)
}

fn challenge_of(verifier: &str) -> String {
    b64url(&znimok_update::sha256::digest(verifier.as_bytes()))
}

/// `application/x-www-form-urlencoded` of one value.
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// A sign-in under way: the address to open in the browser, and the port that waits for Google.
pub struct Pending {
    /// Open this in the system browser.
    pub url: String,
    pub redirect: String,
    verifier: String,
    state: String,
    listener: TcpListener,
}

/// Starts a sign-in: a loopback port of the system's choice, a PKCE pair, a state.
pub fn start(client: &Client) -> std::io::Result<Pending> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let redirect = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    let (verifier, challenge) = pkce();
    let state = b64url(uuid::Uuid::new_v4().as_bytes());
    let scope = SCOPES.join(" ");
    let url = format!(
        "{AUTH_URL}?{}",
        form(&[
            ("client_id", &client.id),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("scope", &scope),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            // A refresh token, and the question shown again after a sign-out.
            ("access_type", "offline"),
            ("prompt", "consent"),
        ])
    );
    Ok(Pending {
        url,
        redirect,
        verifier,
        state,
        listener,
    })
}

/// The page the browser shows when Google sends the person back.
fn page(ok: bool, title: &str, text: &str) -> String {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Znimok</title>\
         <body style=\"font:16px system-ui;margin:12vh auto;max-width:34em;text-align:center\">\
         <h2>{}</h2><p>{}</p></body>",
        title.replace('<', "&lt;"),
        text.replace('<', "&lt;")
    );
    format!(
        "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        if ok { "200 OK" } else { "400 Bad Request" },
        body.len()
    )
}

/// What the browser shows: the words are the app's (its language).
#[derive(Clone, Debug)]
pub struct Words {
    pub done_title: String,
    pub done_text: String,
    pub failed_title: String,
}

impl Pending {
    /// Waits up to `limit` for Google to send the person back; the code, or why there is none
    /// (the person declined, the time ran out, a forged answer).
    pub fn wait(self, limit: Duration, words: &Words) -> Result<Code, String> {
        self.listener
            .set_nonblocking(true)
            .map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(r) = self.answer(stream, words) {
                        return r;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if t0.elapsed() > limit {
                        return Err("the sign-in was not finished in time".into());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    /// One request to the port: Google's answer (`Some`), or anything else (`None`, ignored —
    /// a browser asks for /favicon.ico too).
    fn answer(&self, stream: TcpStream, words: &Words) -> Option<Result<Code, String>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let target = line.split_whitespace().nth(1)?;
        let query = target.strip_prefix("/?")?;
        let get = |k: &str| {
            query.split('&').find_map(|p| {
                let (a, b) = p.split_once('=')?;
                (a == k).then(|| dec(b))
            })
        };
        let mut out = &stream;
        let r = if get("state").as_deref() != Some(self.state.as_str()) {
            Err("the answer did not come from this sign-in".to_string())
        } else if let Some(e) = get("error") {
            Err(if e == "access_denied" {
                "the person did not allow it".into()
            } else {
                format!("Google said: {e}")
            })
        } else {
            Ok(Code {
                code: get("code")?,
                verifier: self.verifier.clone(),
                redirect: self.redirect.clone(),
            })
        };
        let reply = match &r {
            Ok(_) => page(true, &words.done_title, &words.done_text),
            Err(e) => page(false, &words.failed_title, e),
        };
        let _ = out.write_all(reply.as_bytes());
        let _ = out.flush();
        Some(r)
    }
}

/// Google's code, ready to be exchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub code: String,
    verifier: String,
    redirect: String,
}

fn tokens(status: u16, body: &[u8]) -> Result<Tokens, String> {
    if status != 200 {
        let v: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
        return Err(format!(
            "Google: HTTP {status} {}",
            v["error_description"]
                .as_str()
                .or(v["error"].as_str())
                .unwrap_or("")
        )
        .trim_end()
        .to_string());
    }
    serde_json::from_slice(body).map_err(|e| format!("Google's answer: {e}"))
}

/// The code and the verifier for tokens (with the refresh token).
pub fn exchange(http: &dyn Post, client: &Client, code: &Code) -> Result<Tokens, String> {
    let body = form(&[
        ("client_id", &client.id),
        ("client_secret", &client.secret),
        ("code", &code.code),
        ("code_verifier", &code.verifier),
        ("redirect_uri", &code.redirect),
        ("grant_type", "authorization_code"),
    ]);
    let (status, out) = http.post_form(TOKEN_URL, &body)?;
    let t = tokens(status, &out)?;
    if t.refresh_token.is_none() {
        return Err("Google gave no refresh token".into());
    }
    Ok(t)
}

/// A fresh access token for the refresh token.
pub fn refresh(http: &dyn Post, client: &Client, refresh_token: &str) -> Result<Tokens, String> {
    let body = form(&[
        ("client_id", &client.id),
        ("client_secret", &client.secret),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ]);
    let (status, out) = http.post_form(TOKEN_URL, &body)?;
    tokens(status, &out)
}

/// «Sign out»: the refresh token stops working at Google too.
pub fn revoke(http: &dyn Post, token: &str) -> Result<(), String> {
    let (status, _) = http.post_form(REVOKE_URL, &form(&[("token", token)]))?;
    // 400 = already revoked or expired: signed out all the same.
    if status == 200 || status == 400 {
        Ok(())
    } else {
        Err(format!("Google: HTTP {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// RFC 7636, appendix B.
    #[test]
    fn pkce_challenge_is_s256() {
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (v, c) = pkce();
        assert_eq!(v.len(), 43);
        assert_eq!(c, challenge_of(&v));
        assert_ne!(pkce().0, v, "a new pair each time");
    }

    #[test]
    fn form_values_are_encoded() {
        assert_eq!(enc("a b/č&="), "a%20b%2F%C4%8D%26%3D");
        assert_eq!(dec("a%20b%2F%C4%8D%26%3D+x"), "a b/č&= x");
        assert_eq!(dec("100%"), "100%");
    }

    fn words() -> Words {
        Words {
            done_title: "Готово".into(),
            done_text: "Можна закрити".into(),
            failed_title: "Не вдалося".into(),
        }
    }

    fn back(p: &Pending, query: &str) -> std::thread::JoinHandle<String> {
        let port = p
            .redirect
            .trim_end_matches('/')
            .rsplit(':')
            .next()
            .unwrap()
            .to_string();
        let query = query.to_string();
        std::thread::spawn(move || {
            // The favicon first, as a browser does — ignored.
            for q in ["favicon.ico".to_string(), format!("?{query}")] {
                let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
                write!(s, "GET /{q} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut out = String::new();
                let _ = std::io::Read::read_to_string(&mut s, &mut out);
                if q.starts_with('?') {
                    return out;
                }
            }
            String::new()
        })
    }

    /// The whole sign-in round with Google's part played here: the address carries the
    /// challenge, the state and the scope; Google's answer gives the code; a forged state or a
    /// refusal is an error; the browser is told either way.
    #[test]
    fn loopback_round() {
        let client = Client {
            id: "id.apps.googleusercontent.com".into(),
            secret: "s".into(),
        };
        let p = start(&client).unwrap();
        assert!(p.url.starts_with(AUTH_URL));
        assert!(p.url.contains("code_challenge_method=S256"));
        assert!(p.url.contains("drive.file"));
        assert!(!p.url.contains("gmail"), "no sensitive scope");
        let state = p
            .url
            .split("state=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_string();
        let page = back(&p, &format!("state={state}&code=4%2F0AbC&scope=x"));
        let code = p.wait(Duration::from_secs(5), &words()).unwrap();
        assert_eq!(code.code, "4/0AbC");
        assert!(page.join().unwrap().contains("Готово"));

        let p = start(&client).unwrap();
        let page = back(&p, "state=forged&code=x");
        assert!(p.wait(Duration::from_secs(5), &words()).is_err());
        assert!(page.join().unwrap().contains("400"));

        let p = start(&client).unwrap();
        let state = p
            .url
            .split("state=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_string();
        let _ = back(&p, &format!("state={state}&error=access_denied"));
        assert_eq!(
            p.wait(Duration::from_secs(5), &words()).unwrap_err(),
            "the person did not allow it"
        );

        let p = start(&client).unwrap();
        assert!(
            p.wait(Duration::from_millis(300), &words()).is_err(),
            "times out"
        );
    }

    struct Fake {
        sent: Mutex<Vec<(String, String)>>,
        answer: (u16, &'static str),
    }

    impl Post for Fake {
        fn post_form(&self, url: &str, body: &str) -> Result<(u16, Vec<u8>), String> {
            self.sent.lock().unwrap().push((url.into(), body.into()));
            Ok((self.answer.0, self.answer.1.as_bytes().to_vec()))
        }
    }

    #[test]
    fn exchange_refresh_revoke() {
        let client = Client {
            id: "cid".into(),
            secret: "sec".into(),
        };
        let code = Code {
            code: "4/xyz".into(),
            verifier: "ver".into(),
            redirect: "http://127.0.0.1:5555/".into(),
        };
        // The OpenID token's payload: {"email":"a@b.c"}.
        let ok = Fake {
            sent: Default::default(),
            answer: (
                200,
                r#"{"access_token":"at","refresh_token":"rt","expires_in":3599,"id_token":"h.eyJlbWFpbCI6ImFAYi5jIn0.s"}"#,
            ),
        };
        let t = exchange(&ok, &client, &code).unwrap();
        assert_eq!(
            (t.access_token.as_str(), t.refresh_token.as_deref()),
            ("at", Some("rt"))
        );
        assert_eq!(t.email().as_deref(), Some("a@b.c"));
        let (url, body) = ok.sent.lock().unwrap()[0].clone();
        assert_eq!(url, TOKEN_URL);
        assert!(body.contains("code=4%2Fxyz") && body.contains("code_verifier=ver"));
        assert!(body.contains("grant_type=authorization_code"));
        assert!(refresh(&ok, &client, "rt").is_ok());
        assert!(
            ok.sent.lock().unwrap()[1]
                .1
                .contains("grant_type=refresh_token")
        );

        let bad = Fake {
            sent: Default::default(),
            answer: (
                400,
                r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
            ),
        };
        assert_eq!(
            refresh(&bad, &client, "rt").unwrap_err(),
            "Google: HTTP 400 Bad Request"
        );
        assert!(
            revoke(&bad, "rt").is_ok(),
            "already revoked is signed out too"
        );
    }
}
