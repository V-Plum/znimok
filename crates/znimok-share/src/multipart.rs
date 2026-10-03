//! `multipart/form-data` bodies (RFC 7578): text fields and one or more files.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Form {
    boundary: String,
    body: Vec<u8>,
}

impl Form {
    pub fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            boundary: format!("znimok{:x}{:x}", t, N.fetch_add(1, Ordering::Relaxed)),
            body: Vec::new(),
        }
    }

    fn head(&mut self, disposition: &str, content_type: Option<&str>) {
        self.body.extend_from_slice(b"--");
        self.body.extend_from_slice(self.boundary.as_bytes());
        self.body
            .extend_from_slice(b"\r\nContent-Disposition: form-data; ");
        self.body.extend_from_slice(disposition.as_bytes());
        self.body.extend_from_slice(b"\r\n");
        if let Some(ct) = content_type {
            self.body.extend_from_slice(b"Content-Type: ");
            self.body.extend_from_slice(ct.as_bytes());
            self.body.extend_from_slice(b"\r\n");
        }
        self.body.extend_from_slice(b"\r\n");
    }

    pub fn text(mut self, name: &str, value: &str) -> Self {
        self.head(&format!("name=\"{}\"", quote(name)), None);
        self.body.extend_from_slice(value.as_bytes());
        self.body.extend_from_slice(b"\r\n");
        self
    }

    /// A text field with its own type (a webhook's JSON metadata).
    pub fn typed(mut self, name: &str, content_type: &str, value: &[u8]) -> Self {
        self.head(&format!("name=\"{}\"", quote(name)), Some(content_type));
        self.body.extend_from_slice(value);
        self.body.extend_from_slice(b"\r\n");
        self
    }

    pub fn file(mut self, name: &str, file_name: &str, content_type: &str, bytes: &[u8]) -> Self {
        // The name twice: plain (ASCII, for old receivers) and RFC 5987 (UTF-8, Cyrillic).
        let ascii: String = file_name
            .chars()
            .map(|c| {
                if c.is_ascii() && c != '"' && c != '\\' && !c.is_control() {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let disposition = if ascii == file_name {
            format!("name=\"{}\"; filename=\"{}\"", quote(name), ascii)
        } else {
            format!(
                "name=\"{}\"; filename=\"{}\"; filename*=UTF-8''{}",
                quote(name),
                ascii,
                percent(file_name)
            )
        };
        self.head(&disposition, Some(content_type));
        self.body.extend_from_slice(bytes);
        self.body.extend_from_slice(b"\r\n");
        self
    }

    /// The body and its `Content-Type` (with the boundary).
    pub fn finish(mut self) -> (Vec<u8>, String) {
        self.body.extend_from_slice(b"--");
        self.body.extend_from_slice(self.boundary.as_bytes());
        self.body.extend_from_slice(b"--\r\n");
        let ct = format!("multipart/form-data; boundary={}", self.boundary);
        (self.body, ct)
    }
}

fn quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Percent-encoding of everything but unreserved characters (for URLs and `filename*`).
pub fn percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_with_a_cyrillic_file_name() {
        let (body, ct) = Form::new()
            .text("chat_id", "42")
            .file("document", "Знімок 1.png", "image/png", b"\x89PNG")
            .finish();
        let b = String::from_utf8_lossy(&body);
        let boundary = ct.split("boundary=").nth(1).unwrap();
        assert!(ct.starts_with("multipart/form-data; boundary="));
        assert!(b.starts_with(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"chat_id\"\r\n\r\n42\r\n"
        )));
        assert!(b.contains("filename=\"______ 1.png\"; filename*=UTF-8''%D0%97%D0%BD%D1%96%D0%BC%D0%BE%D0%BA%201.png"));
        assert!(b.contains("Content-Type: image/png\r\n\r\n"));
        assert!(b.ends_with(&format!("--{boundary}--\r\n")));
    }
}
