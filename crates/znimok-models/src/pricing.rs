//! Claude API prices, USD per million tokens (platform.claude.com/docs/en/about-claude/pricing,
//! checked 29.09.2026). A model missing here has no estimate — the meter still counts its tokens.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    /// 5-minute cache write.
    pub cache_write: f64,
    pub cache_read: f64,
    pub output: f64,
}

/// Default for the assistant (owner, 28.09: the current Sonnet; Sonnet 5.5 since 29.09).
pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

/// Offered in the settings: (id, name).
pub const MODELS: &[(&str, &str)] = &[
    ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
    ("claude-opus-5-5", "Claude Opus 5.5"),
    ("claude-haiku-4-5", "Claude Haiku 4.5"),
];

pub fn price(model: &str) -> Option<Price> {
    let p = |input: f64, cache_write: f64, cache_read: f64, output: f64| Price {
        input,
        cache_write,
        cache_read,
        output,
    };
    // Dated snapshot ids resolve to their family.
    let m = model.strip_suffix("-20251001").unwrap_or(model);
    Some(match m {
        "claude-fable-5-1" => p(10.0, 12.5, 0.25, 50.0),
        "claude-fable-5" => p(10.0, 12.5, 1.0, 50.0),
        "claude-opus-5-5" => p(4.0, 5.0, 0.20, 20.0),
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" => {
            p(5.0, 6.25, 0.50, 25.0)
        }
        "claude-sonnet-5-5" | "claude-sonnet-5" => p(2.0, 2.5, 0.20, 10.0),
        "claude-sonnet-4-6" | "claude-sonnet-4-5" => p(3.0, 3.75, 0.30, 15.0),
        "claude-haiku-4-5" => p(1.0, 1.25, 0.10, 5.0),
        _ => return None,
    })
}

/// Tokens of one reply, as the API reports them in `usage`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, o: &Usage) {
        self.input_tokens += o.input_tokens;
        self.output_tokens += o.output_tokens;
        self.cache_creation_input_tokens += o.cache_creation_input_tokens;
        self.cache_read_input_tokens += o.cache_read_input_tokens;
    }

    /// USD, or `None` for a model without a known price.
    pub fn cost(&self, model: &str) -> Option<f64> {
        let p = price(model)?;
        Some(
            (self.input_tokens as f64 * p.input
                + self.cache_creation_input_tokens as f64 * p.cache_write
                + self.cache_read_input_tokens as f64 * p.cache_read
                + self.output_tokens as f64 * p.output)
                / 1_000_000.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs() {
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 1_000_000,
        };
        let c = u.cost("claude-sonnet-5-5").unwrap();
        assert!((c - (2.0 + 1.0 + 0.2)).abs() < 1e-9, "{c}");
        assert!((u.cost("claude-opus-5-5").unwrap() - (4.0 + 2.0 + 0.2)).abs() < 1e-9);
        assert_eq!(u.cost("claude-unknown-9"), None);
        assert!(price("claude-haiku-4-5-20251001").is_some());
        assert!(MODELS.iter().all(|(id, _)| price(id).is_some()));
        assert!(price(DEFAULT_MODEL).is_some());
    }
}
