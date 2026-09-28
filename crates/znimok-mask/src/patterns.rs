//! What counts as sensitive in a line of text. Each hit is the span to hide — for `key: value`
//! and URL parameters only the value, so the label stays readable.
//!
//! Numbers are only reported when their checksum holds (cards: Luhn, IBAN: mod 97), phones when
//! they have a plausible count of digits — fewer false alarms on screenshots full of numbers.

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// API keys and tokens with a known shape (Anthropic, OpenAI, GitHub, Slack, AWS, Google,
    /// Stripe, JWT, private key blocks).
    Token,
    /// The value after `password:`, `token=`, `api_key:`…
    Secret,
    /// A secret URL parameter (`?token=…`, the Little Helpers RepMask list).
    UrlSecret,
    Email,
    Phone,
    Card,
    Iban,
    Face,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub kind: Kind,
    /// Byte range in the line.
    pub start: usize,
    pub end: usize,
}

/// URL parameter names whose values are secret (LH `RepSecretName`).
const SECRET_PARAMS: &str = "token|secret|password|passwd|pwd|auth|key|sig|session|sid|code|jwt|bearer|cookie|credential|otp|ticket|nonce";

static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"sk-ant-[A-Za-z0-9_\-]{16,}",
        r"|sk-(?:proj-)?[A-Za-z0-9_\-]{20,}",
        r"|gh[pousr]_[A-Za-z0-9]{30,}",
        r"|github_pat_[A-Za-z0-9_]{40,}",
        r"|xox[abprs]-[A-Za-z0-9\-]{10,}",
        r"|\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
        r"|\bAIza[0-9A-Za-z_\-]{35}\b",
        r"|\b[rs]k_(?:live|test)_[0-9A-Za-z]{16,}",
        r"|\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
        r"|-----BEGIN [A-Z ]*PRIVATE KEY-----",
    ))
    .expect("token patterns")
});

/// `password: hunter2`, `API_KEY=abc`, `пароль — qwerty`: group 1 is the value.
static KEY_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(?:password|passwd|pwd|pass|secret|token|api[_\- ]?key|access[_\- ]?key|client[_\- ]?secret|auth|пароль|токен|ключ)\b\s*["']?\s*[:=—–-]\s*["']?([^\s"',;]{3,})"#,
    )
    .expect("key/value pattern")
});

static URL_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)[?&#][A-Za-z0-9_.\-]*(?:{SECRET_PARAMS})[A-Za-z0-9_.\-]*=([^&#\s]+)"
    ))
    .expect("url pattern")
});

static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}")
        .expect("email")
});

static CARD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d(?:[ \-]?\d){12,18}\b").expect("card"));

static IBAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Z]{2}\d{2}(?:[ ]?[A-Z0-9]){11,30}\b").expect("iban"));

/// `+380 67 123 45 67`, `(067) 123-45-67`, `+1 415 555 0100`. No dots: dates and versions
/// (`28.09.2026 22:15`, `1.2.3`) look like that.
static PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:\+|\b)\d[\d ()\-]{7,18}\d\b").expect("phone"));

pub fn luhn(digits: &str) -> bool {
    let d: Vec<u32> = digits.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&d.len()) {
        return false;
    }
    let sum: u32 = d
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| {
            if i % 2 == 1 {
                let y = x * 2;
                if y > 9 { y - 9 } else { y }
            } else {
                x
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

pub fn iban_ok(s: &str) -> bool {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !(15..=34).contains(&s.len()) {
        return false;
    }
    let (head, tail) = s.split_at(4);
    let mut rem: u32 = 0;
    for c in tail.chars().chain(head.chars()) {
        let v = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'A'..='Z' => c as u32 - 'A' as u32 + 10,
            _ => return false,
        };
        for digit in v.to_string().chars() {
            rem = (rem * 10 + digit.to_digit(10).unwrap_or(0)) % 97;
        }
    }
    rem == 1
}

/// All sensitive spans of `line`, longest first where they overlap (a token inside a URL
/// parameter is reported once).
pub fn find(line: &str) -> Vec<Hit> {
    let mut hits: Vec<Hit> = Vec::new();
    let mut push = |kind, start, end| hits.push(Hit { kind, start, end });
    for m in TOKENS.find_iter(line) {
        push(Kind::Token, m.start(), m.end());
    }
    for c in URL_SECRET.captures_iter(line) {
        let v = c.get(1).expect("group");
        push(Kind::UrlSecret, v.start(), v.end());
    }
    for c in KEY_VALUE.captures_iter(line) {
        let v = c.get(1).expect("group");
        push(Kind::Secret, v.start(), v.end());
    }
    for m in EMAIL.find_iter(line) {
        push(Kind::Email, m.start(), m.end());
    }
    for m in CARD.find_iter(line) {
        if luhn(m.as_str()) {
            push(Kind::Card, m.start(), m.end());
        }
    }
    for m in IBAN.find_iter(line) {
        if iban_ok(m.as_str()) {
            push(Kind::Iban, m.start(), m.end());
        }
    }
    for m in PHONE.find_iter(line) {
        let s = m.as_str();
        let digits = s.chars().filter(char::is_ascii_digit).count();
        if (9..=15).contains(&digits) {
            push(Kind::Phone, m.start(), m.end());
        }
    }
    // Keep the widest of overlapping hits; a card also matches the phone shape.
    hits.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
    let mut out: Vec<Hit> = Vec::new();
    for h in hits {
        match out.last_mut() {
            Some(last) if h.start < last.end => {
                if h.end > last.end {
                    last.end = h.end;
                }
            }
            _ => out.push(h),
        }
    }
    out
}

/// The text with every sensitive span replaced by `•••` — for logs, reports and text sent to a
/// model.
pub fn mask_text(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let mut out = String::with_capacity(line.len());
            let mut at = 0;
            for h in find(line) {
                out.push_str(&line[at..h.start]);
                out.push_str("•••");
                at = h.end;
            }
            out.push_str(&line[at..]);
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(Kind, &str)> {
        find(line)
            .into_iter()
            .map(|h| (h.kind, &line[h.start..h.end]))
            .collect()
    }

    #[test]
    fn tokens_by_shape() {
        let k = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv";
        assert_eq!(kinds(&format!("ключ {k} тут")), [(Kind::Token, k)]);
        let gh = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
        assert_eq!(kinds(gh), [(Kind::Token, gh)]);
        assert_eq!(
            kinds("AKIAIOSFODNN7EXAMPLE"),
            [(Kind::Token, "AKIAIOSFODNN7EXAMPLE")]
        );
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.abcdefghijklmnop";
        assert_eq!(kinds(jwt), [(Kind::Token, jwt)]);
        assert_eq!(kinds("-----BEGIN RSA PRIVATE KEY-----")[0].0, Kind::Token);
    }

    #[test]
    fn only_the_value_of_key_value() {
        assert_eq!(kinds("password: hunter2!"), [(Kind::Secret, "hunter2!")]);
        assert_eq!(
            kinds("API_KEY=\"abc123xyz\""),
            [(Kind::Secret, "abc123xyz")]
        );
        assert_eq!(kinds("Пароль — Київ2026"), [(Kind::Secret, "Київ2026")]);
        assert!(kinds("password manager is great").is_empty());
    }

    #[test]
    fn url_parameters_from_the_lh_list() {
        let l = "https://x.test/cb?user=vadym&access_token=AbC123&x=1";
        assert_eq!(kinds(l), [(Kind::UrlSecret, "AbC123")]);
        assert!(kinds("https://x.test/?page=2&sort=asc").is_empty());
    }

    #[test]
    fn personal_data_with_checks() {
        assert_eq!(
            kinds("пишіть на v.v.plum@gmail.com"),
            [(Kind::Email, "v.v.plum@gmail.com")]
        );
        assert_eq!(
            kinds("4111 1111 1111 1111"),
            [(Kind::Card, "4111 1111 1111 1111")]
        );
        assert!(
            kinds("4111 1111 1111 1112")
                .iter()
                .all(|(k, _)| *k != Kind::Card)
        );
        let iban = "UA21 3223 1300 0002 6007 2335 6600 1";
        assert_eq!(kinds(iban), [(Kind::Iban, iban)]);
        assert_eq!(
            kinds("тел. +380 67 123 45 67"),
            [(Kind::Phone, "+380 67 123 45 67")]
        );
        assert_eq!(kinds("(067) 123-45-67")[0].0, Kind::Phone);
    }

    #[test]
    fn ordinary_numbers_are_left_alone() {
        for l in [
            "28.09.2026 22:15:00",
            "версія 1.2.3",
            "1920×1080",
            "Разом: 12 345,67 грн",
            "Order 1234567",
        ] {
            assert!(kinds(l).is_empty(), "{l}: {:?}", kinds(l));
        }
    }

    #[test]
    fn checksums() {
        assert!(luhn("4539 1488 0343 6467"));
        assert!(!luhn("1234 5678 9012 3456"));
        assert!(iban_ok("GB82 WEST 1234 5698 7654 32"));
        assert!(!iban_ok("GB82 WEST 1234 5698 7654 33"));
    }

    #[test]
    fn mask_text_keeps_labels() {
        assert_eq!(
            mask_text("login: vadym\npassword: hunter2\nhttps://a.b/?sig=XYZ&p=1\n"),
            "login: vadym\npassword: •••\nhttps://a.b/?sig=•••&p=1\n"
        );
    }
}
