//! Claude with the user's own key (BYOK), Messages API.
//!
//! Two steps on purpose: [`Outgoing::prepare`] builds exactly what will leave the machine (text,
//! the downscaled pictures, an estimate of tokens and cost) — the app shows it when the feature
//! is in «ask» mode (owner: preview before sending, with masking done by the caller) — then
//! [`Client::send`] sends that very value. Every reply is added to the local [`Meter`].

use crate::Rgba;
use crate::http::{HttpError, Transport};
use crate::image_prep::{self, MAX_IMAGES, PrepError, Prepared};
use crate::meter::Meter;
use crate::pricing::{Usage, price};
use serde_json::{Value, json};
use std::fmt;
use std::time::Duration;
use znimok_settings::{Secret, Vault};

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2023-06-01";
/// For development only (owner, 28.09); the app reads the OS store.
pub const KEY_ENV: &str = "ANTHROPIC_API_KEY";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    Env,
    Store,
}

/// The key: `ANTHROPIC_API_KEY` first (development), then the OS store.
pub fn api_key(vault: &Vault) -> Result<Option<(String, KeySource)>, AiError> {
    if let Some(k) = std::env::var(KEY_ENV).ok().filter(|k| !k.trim().is_empty()) {
        return Ok(Some((k.trim().to_string(), KeySource::Env)));
    }
    vault
        .get(Secret::AnthropicApiKey)
        .map(|k| k.map(|k| (k, KeySource::Store)))
        .map_err(|e| AiError::Secret(e.to_string()))
}

/// For the settings page: «•••• abcd» (owner, 28.09). Short keys show nothing of themselves.
pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    if chars.len() < 12 {
        return "••••".into();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("•••• {tail}")
}

/// What to ask.
#[derive(Clone, Debug, Default)]
pub struct Ask {
    pub model: String,
    pub system: Option<String>,
    /// Pictures go first (the API reads them better before the text).
    pub images: Vec<Rgba>,
    pub text: String,
    pub max_tokens: u32,
}

/// Exactly what will be sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    pub model: String,
    pub system: Option<String>,
    pub images: Vec<Prepared>,
    pub text: String,
    pub max_tokens: u32,
    /// Pictures exactly, text roughly (≈ 3 characters a token for mixed Ukrainian/English).
    pub estimated_input_tokens: u64,
    /// Upper bound: estimated input + `max_tokens` of output; `None` without a known price.
    pub estimated_max_usd: Option<f64>,
}

impl Outgoing {
    pub fn prepare(ask: &Ask) -> Result<Self, AiError> {
        if ask.images.len() > MAX_IMAGES {
            return Err(AiError::TooLarge);
        }
        let images = ask
            .images
            .iter()
            .map(image_prep::prepare)
            .collect::<Result<Vec<_>, _>>()
            .map_err(AiError::Image)?;
        let text_chars =
            ask.text.chars().count() + ask.system.as_deref().map_or(0, |s| s.chars().count());
        let estimated_input_tokens =
            images.iter().map(|i| i.visual_tokens).sum::<u64>() + (text_chars as u64).div_ceil(3);
        let estimated_max_usd = price(&ask.model).map(|p| {
            (estimated_input_tokens as f64 * p.input + ask.max_tokens as f64 * p.output) / 1e6
        });
        Ok(Self {
            model: ask.model.clone(),
            system: ask.system.clone(),
            images,
            text: ask.text.clone(),
            max_tokens: ask.max_tokens.max(1),
            estimated_input_tokens,
            estimated_max_usd,
        })
    }

    pub fn body(&self) -> Vec<u8> {
        let mut content: Vec<Value> = self
            .images
            .iter()
            .map(|i| {
                json!({"type": "image", "source": {
                    "type": "base64", "media_type": i.media_type, "data": i.base64}})
            })
            .collect();
        content.push(json!({"type": "text", "text": self.text}));
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [{"role": "user", "content": content}],
        });
        if let Some(s) = &self.system {
            body["system"] = json!(s);
        }
        serde_json::to_vec(&body).expect("JSON")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub text: String,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: Usage,
    pub usd: Option<f64>,
}

#[derive(Debug, PartialEq)]
pub enum AiError {
    /// No key in the OS store (and no `ANTHROPIC_API_KEY`).
    NoKey,
    /// The key was refused (401 / 403).
    BadKey(String),
    RateLimited {
        retry_after: Option<u64>,
    },
    /// 529: the service is busy; try later.
    Overloaded {
        retry_after: Option<u64>,
    },
    /// Request or picture too large (413, or more than 20 pictures).
    TooLarge,
    /// 400: the request itself (model name, parameters).
    Invalid(String),
    Server(u16, String),
    Network(HttpError),
    Image(PrepError),
    Secret(String),
    /// The answer is not what the API documents.
    Protocol(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoKey => write!(f, "no Anthropic API key"),
            Self::BadKey(m) => write!(f, "the API key was refused: {m}"),
            Self::RateLimited { .. } => write!(f, "too many requests"),
            Self::Overloaded { .. } => write!(f, "the service is overloaded"),
            Self::TooLarge => write!(f, "the request is too large"),
            Self::Invalid(m) => write!(f, "invalid request: {m}"),
            Self::Server(s, m) => write!(f, "server error {s}: {m}"),
            Self::Network(e) => write!(f, "{e}"),
            Self::Image(e) => write!(f, "picture: {e:?}"),
            Self::Secret(m) => write!(f, "key store: {m}"),
            Self::Protocol(m) => write!(f, "unexpected answer: {m}"),
        }
    }
}

impl std::error::Error for AiError {}

pub struct Client {
    transport: Box<dyn Transport>,
    key: String,
    meter: Option<Meter>,
    pub timeout: Duration,
}

impl Client {
    pub fn new(key: String, transport: Box<dyn Transport>, meter: Option<Meter>) -> Self {
        Self {
            transport,
            key,
            meter,
            timeout: Duration::from_secs(120),
        }
    }

    /// The OS transport, the key from the OS store (or the env for development), the default meter.
    pub fn from_system() -> Result<Self, AiError> {
        let (key, _) = api_key(&Vault::default())?.ok_or(AiError::NoKey)?;
        Ok(Self::new(key, crate::http::system(), Meter::open_default()))
    }

    /// Sends a prepared request; `month` (`YYYY-MM`) is where the meter books it.
    pub fn send(&self, out: &Outgoing, month: &str) -> Result<Answer, AiError> {
        let v = self.messages(&serde_json::from_slice(&out.body()).expect("JSON"), month)?;
        let text = v["content"]
            .as_array()
            .ok_or_else(|| AiError::Protocol("no content".into()))?
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        let usage: Usage = serde_json::from_value(v["usage"].clone()).unwrap_or_default();
        let model = v["model"].as_str().unwrap_or(&out.model).to_string();
        Ok(Answer {
            text,
            usd: usage.cost(&model),
            model,
            stop_reason: v["stop_reason"].as_str().map(str::to_string),
            usage,
        })
    }

    /// Any Messages API request (tools, several turns — the assistant, ZK-73); returns the
    /// response body. The meter books its usage under `month`.
    pub fn messages(&self, body: &Value, month: &str) -> Result<Value, AiError> {
        let bytes = serde_json::to_vec(body).map_err(|e| AiError::Protocol(e.to_string()))?;
        let resp = self
            .transport
            .post_json(
                API_URL,
                &[("x-api-key", &self.key), ("anthropic-version", API_VERSION)],
                &bytes,
                self.timeout,
            )
            .map_err(AiError::Network)?;
        let v: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
        let message = || v["error"]["message"].as_str().unwrap_or("").to_string();
        match resp.status {
            200 => {}
            401 | 403 => return Err(AiError::BadKey(message())),
            413 => return Err(AiError::TooLarge),
            429 => {
                return Err(AiError::RateLimited {
                    retry_after: resp.retry_after,
                });
            }
            529 => {
                return Err(AiError::Overloaded {
                    retry_after: resp.retry_after,
                });
            }
            400..=499 => return Err(AiError::Invalid(message())),
            s => return Err(AiError::Server(s, message())),
        }
        if !v["content"].is_array() {
            return Err(AiError::Protocol("no content".into()));
        }
        let usage: Usage = serde_json::from_value(v["usage"].clone()).unwrap_or_default();
        let model = v["model"]
            .as_str()
            .or(body["model"].as_str())
            .unwrap_or("")
            .to_string();
        if let Some(m) = &self.meter {
            // Counting must not lose the answer.
            let _ = m.record(month, &model, &usage);
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    use std::sync::Mutex;

    /// Headers and JSON body of one request.
    type Seen = (Vec<(String, String)>, Value);

    struct Fake {
        status: u16,
        body: String,
        seen: Mutex<Vec<Seen>>,
    }

    impl Transport for Fake {
        fn post_json(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
            _: Duration,
        ) -> Result<Response, HttpError> {
            assert_eq!(url, API_URL);
            self.seen.lock().unwrap().push((
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                serde_json::from_slice(body).unwrap(),
            ));
            Ok(Response {
                status: self.status,
                body: self.body.clone().into_bytes(),
                retry_after: Some(7),
            })
        }
    }

    fn fake(status: u16, body: &str) -> std::sync::Arc<Fake> {
        std::sync::Arc::new(Fake {
            status,
            body: body.into(),
            seen: Mutex::new(Vec::new()),
        })
    }

    struct Shared(std::sync::Arc<Fake>);
    impl Transport for Shared {
        fn post_json(
            &self,
            u: &str,
            h: &[(&str, &str)],
            b: &[u8],
            t: Duration,
        ) -> Result<Response, HttpError> {
            self.0.post_json(u, h, b, t)
        }
    }

    fn ask() -> Ask {
        Ask {
            model: "claude-sonnet-5-5".into(),
            system: Some("Коротко.".into()),
            images: vec![Rgba::new(3000, 1500, vec![255; 3000 * 1500 * 4]).unwrap()],
            text: "Що на знімку?".into(),
            max_tokens: 500,
        }
    }

    #[test]
    fn outgoing_is_what_is_sent() {
        let out = Outgoing::prepare(&ask()).unwrap();
        assert_eq!(out.images.len(), 1);
        assert_eq!(out.images[0].width, 2576);
        let body: Value = serde_json::from_slice(&out.body()).unwrap();
        let content = &body["messages"][0]["content"];
        assert_eq!(content[0]["type"], "image", "pictures first");
        assert_eq!(content[0]["source"]["data"], out.images[0].base64);
        assert_eq!(content[1]["text"], "Що на знімку?");
        assert_eq!(body["system"], "Коротко.");
        assert_eq!(body["max_tokens"], 500);
        assert!(out.estimated_input_tokens > out.images[0].visual_tokens);
        let usd = out.estimated_max_usd.unwrap();
        assert!(usd > 0.005 && usd < 0.02, "{usd}");
    }

    #[test]
    fn a_reply_is_parsed_and_counted() {
        let f = fake(
            200,
            r#"{"model":"claude-sonnet-5-5","stop_reason":"end_turn",
                "content":[{"type":"text","text":"Вікно "},{"type":"text","text":"налаштувань."}],
                "usage":{"input_tokens":4000,"output_tokens":20}}"#,
        );
        let meter_path =
            std::env::temp_dir().join(format!("znimok-ai-meter-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&meter_path);
        let c = Client::new(
            "sk-ant-test".into(),
            Box::new(Shared(f.clone())),
            Some(Meter::new(&meter_path)),
        );
        let a = c
            .send(&Outgoing::prepare(&ask()).unwrap(), "2026-09")
            .unwrap();
        assert_eq!(a.text, "Вікно налаштувань.");
        assert_eq!(a.stop_reason.as_deref(), Some("end_turn"));
        assert!((a.usd.unwrap() - (4000.0 * 2.0 + 20.0 * 10.0) / 1e6).abs() < 1e-12);
        let seen = f.seen.lock().unwrap();
        let headers = &seen[0].0;
        assert!(headers.contains(&("x-api-key".into(), "sk-ant-test".into())));
        assert!(headers.contains(&("anthropic-version".into(), API_VERSION.into())));
        let m = Meter::new(&meter_path).months();
        assert_eq!(m["2026-09"]["claude-sonnet-5-5"].requests, 1);
        let _ = std::fs::remove_file(&meter_path);
    }

    #[test]
    fn errors_are_told_apart() {
        let out = Outgoing::prepare(&Ask {
            model: "claude-haiku-4-5".into(),
            text: "x".into(),
            max_tokens: 1,
            ..Default::default()
        })
        .unwrap();
        let send = |status, body: &str| {
            Client::new("k".into(), Box::new(Shared(fake(status, body))), None)
                .send(&out, "2026-09")
        };
        let err = r#"{"type":"error","error":{"type":"x","message":"bad model"}}"#;
        assert_eq!(send(401, err), Err(AiError::BadKey("bad model".into())));
        assert_eq!(send(400, err), Err(AiError::Invalid("bad model".into())));
        assert_eq!(
            send(429, err),
            Err(AiError::RateLimited {
                retry_after: Some(7)
            })
        );
        assert_eq!(
            send(529, err),
            Err(AiError::Overloaded {
                retry_after: Some(7)
            })
        );
        assert_eq!(send(413, err), Err(AiError::TooLarge));
        assert_eq!(
            send(500, err),
            Err(AiError::Server(500, "bad model".into()))
        );
        assert!(matches!(send(200, "{}"), Err(AiError::Protocol(_))));
    }

    #[test]
    fn keys_are_masked() {
        assert_eq!(mask("sk-ant-api03-abcdefgh1234"), "•••• 1234");
        assert_eq!(mask("short"), "••••");
    }

    #[test]
    fn too_many_pictures() {
        let a = Ask {
            images: vec![Rgba::new(1, 1, vec![0; 4]).unwrap(); 21],
            ..ask()
        };
        assert_eq!(Outgoing::prepare(&a), Err(AiError::TooLarge));
    }
}
