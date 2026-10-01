//! Hiding sensitive values of the DevTools log on export (owner, 30.09: the log is written in full;
//! what to hide is the person's choice — in Settings and in the export sheet).
//!
//! A value is hidden when its key is on the list (headers, JSON keys, form fields, dataLayer
//! keys: `authorization`, `cookie`, `email`, …) or when the text itself looks like a secret
//! (`znimok_mask::mask_text`: tokens of known shape, `password=…`, secret URL parameters,
//! e-mails, phones, cards, IBANs). JSON inside strings (a payload, a response) is masked as
//! JSON. Times, methods, statuses and the like are never touched.

use serde_json::Value;

/// The keys hidden by default; the person edits the list in Settings → Recording.
pub const DEFAULT_KEYS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "session",
    "email",
    "phone",
    "user_id",
    "card",
    "cvv",
    "iban",
];

/// What stands for a hidden value.
pub const HIDDEN: &str = "•••";

/// The fields of an event that are its shape, not its data.
const STRUCTURAL: &[&str] = &[
    "k",
    "s",
    "t",
    "ms",
    "method",
    "status",
    "statusText",
    "type",
    "mime",
    "proto",
    "dur",
    "timing",
    "id",
    "b",
    "lvl",
    "ev",
    "op",
    "dir",
    "size",
    "cache",
    "pre",
    "frame",
    "cut",
    "b64",
    "bodyCut",
    "canceled",
    "line",
    "col",
];

/// A key in comparable form: lower case, without `-`, `_`, spaces.
fn norm(k: &str) -> String {
    k.chars()
        .filter(|c| !matches!(c, '-' | '_' | ' ' | '.'))
        .flat_map(char::to_lowercase)
        .collect()
}

pub struct Rules {
    keys: Vec<String>,
}

impl Rules {
    pub fn new<S: AsRef<str>>(keys: &[S]) -> Self {
        Self {
            keys: keys
                .iter()
                .map(|k| norm(k.as_ref()))
                .filter(|k| !k.is_empty())
                .collect(),
        }
    }

    /// Whether a key's value is hidden: the same key, or a longer one containing it (`x-auth-token`
    /// holds `token`, `user_email` holds `email`); keys of four letters or fewer only as they are.
    pub fn hides(&self, key: &str) -> bool {
        let k = norm(key);
        self.keys
            .iter()
            .any(|x| k == *x || (x.chars().count() > 4 && k.contains(x.as_str())))
    }

    /// One event of the log, in place: how many values were hidden.
    pub fn event(&self, ev: &mut Value) -> usize {
        let mut n = 0;
        let binary = ev["b64"] == Value::Bool(true);
        if let Some(o) = ev.as_object_mut() {
            for (k, v) in o.iter_mut() {
                if STRUCTURAL.contains(&k.as_str()) || (binary && k == "body") {
                    continue;
                }
                n += self.value(v);
            }
        }
        n
    }

    /// A value anywhere in an event: objects by their keys, strings by their text.
    fn value(&self, v: &mut Value) -> usize {
        match v {
            Value::Object(o) => {
                let mut n = 0;
                for (k, x) in o.iter_mut() {
                    if self.hides(k) && !x.is_null() && *x != Value::String(HIDDEN.into()) {
                        *x = Value::String(HIDDEN.into());
                        n += 1;
                    } else {
                        n += self.value(x);
                    }
                }
                n
            }
            Value::Array(a) => a.iter_mut().map(|x| self.value(x)).sum(),
            Value::String(s) => {
                let (t, n) = self.text(s);
                if n > 0 {
                    *s = t;
                }
                n
            }
            _ => 0,
        }
    }

    /// A text: JSON as JSON, a form's fields by their names, then the secrets in the text.
    fn text(&self, s: &str) -> (String, usize) {
        let t = s.trim_start();
        if (t.starts_with('{') || t.starts_with('['))
            && let Ok(mut j) = serde_json::from_str::<Value>(s)
        {
            let n = self.value(&mut j);
            return if n > 0 {
                (j.to_string(), n)
            } else {
                (s.to_string(), 0)
            };
        }
        // A form: field by field — by its name, then its value's text.
        if !s.is_empty()
            && !s.contains(char::is_whitespace)
            && s.contains('=')
            && s.split('&').all(|p| p.contains('='))
        {
            let mut n = 0;
            let out = s
                .split('&')
                .map(|p| {
                    let (k, v) = p.split_once('=').unwrap_or((p, ""));
                    if self.hides(k) && !v.is_empty() {
                        n += 1;
                        format!("{k}={HIDDEN}")
                    } else {
                        let (t, m) = Self::patterns(v);
                        n += m;
                        format!("{k}={t}")
                    }
                })
                .collect::<Vec<_>>()
                .join("&");
            return (out, n);
        }
        Self::patterns(s)
    }

    /// The secrets a text holds by their look: the text with them hidden, and how many.
    fn patterns(s: &str) -> (String, usize) {
        let masked = znimok_mask::mask_text(s);
        let n = masked
            .matches(HIDDEN)
            .count()
            .saturating_sub(s.matches(HIDDEN).count());
        (masked, n)
    }
}

/// Every event of a log: how many values were hidden in all.
pub fn events(events: &mut [Value], rules: &Rules) -> usize {
    events.iter_mut().map(|e| rules.event(e)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_headers_json_forms_and_text() {
        let r = Rules::new(DEFAULT_KEYS);
        let mut ev = json!({
            "k": "net", "s": 0, "method": "POST", "status": 200, "dur": 12,
            "url": "https://example.org/api?token=abcdef123456&page=2",
            "reqHeaders": {"Authorization": "Bearer xyz", "Accept": "*/*", "X-Api-Key": "k1"},
            "postData": "{\"user\":{\"email\":\"a@example.org\",\"name\":\"Олена\"},\"n\":1}",
            "body": "password=hunter2&lang=uk",
        });
        let n = r.event(&mut ev);
        assert_eq!(ev["reqHeaders"]["Authorization"], HIDDEN);
        assert_eq!(ev["reqHeaders"]["X-Api-Key"], HIDDEN);
        assert_eq!(ev["reqHeaders"]["Accept"], "*/*");
        let post: Value = serde_json::from_str(ev["postData"].as_str().unwrap()).unwrap();
        assert_eq!(post["user"]["email"], HIDDEN);
        assert_eq!(post["user"]["name"], "Олена");
        assert_eq!(ev["body"], "password=•••&lang=uk");
        assert!(!ev["url"].as_str().unwrap().contains("abcdef123456"));
        assert!(ev["url"].as_str().unwrap().contains("page=2"));
        // Shape untouched.
        assert_eq!(
            (ev["method"].clone(), ev["status"].clone()),
            (json!("POST"), json!(200))
        );
        assert_eq!(n, 5);
    }

    #[test]
    fn datalayer_and_console() {
        let r = Rules::new(&["user_id", "email"]);
        let mut dl = json!({"k": "dl", "ev": "purchase", "data": {"event": "purchase", "userId": 7, "value": 42,
            "contact": {"user_email": "b@example.org"}}});
        assert_eq!(r.event(&mut dl), 2);
        assert_eq!(dl["data"]["userId"], HIDDEN);
        assert_eq!(dl["data"]["contact"]["user_email"], HIDDEN);
        assert_eq!(dl["data"]["value"], 42);
        assert_eq!(dl["ev"], "purchase");
        // A console line with an e-mail in its text: the text patterns catch it.
        let mut c = json!({"k": "console", "text": "signed in as c@example.org", "args": ["signed in as c@example.org"]});
        assert_eq!(r.event(&mut c), 2);
        assert!(!c["text"].as_str().unwrap().contains("c@example.org"));
        // A short key matches only as itself.
        let short = Rules::new(&["card"]);
        assert!(short.hides("Card") && !short.hides("cardinal"));
    }
}
