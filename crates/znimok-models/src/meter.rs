//! Local spending counter: tokens and estimated USD per calendar month and model, kept in
//! `<data>/ai-usage.json`. Only numbers — no prompts, no pictures. Shown in the settings next to
//! the key («цього місяця ≈ $0.42»).

use crate::pricing::Usage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelMonth {
    pub requests: u64,
    pub usage: Usage,
    /// Sum of the estimates; `None` once a request of a model without a price was counted.
    pub usd: Option<f64>,
}

/// `"2026-09"` → model → totals.
pub type Months = BTreeMap<String, BTreeMap<String, ModelMonth>>;

pub struct Meter {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Meter {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// `<data>/ai-usage.json` of the standard folders.
    pub fn open_default() -> Option<Self> {
        znimok_settings::Dirs::system().map(|d| Self::new(d.data.join("ai-usage.json")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn months(&self) -> Months {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Adds one reply to month `month` (`YYYY-MM`, local time — the caller knows the clock).
    pub fn record(&self, month: &str, model: &str, u: &Usage) -> std::io::Result<()> {
        let _g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut all = self.months();
        let m = all
            .entry(month.to_string())
            .or_default()
            .entry(model.to_string())
            .or_insert_with(|| ModelMonth {
                usd: Some(0.0),
                ..Default::default()
            });
        m.requests += 1;
        m.usage.add(u);
        m.usd = match (m.usd, u.cost(model)) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&all)?)?;
        std::fs::rename(&tmp, &self.path)
    }

    /// Estimated USD of a month over all models; `None` if some model had no price.
    pub fn month_usd(&self, month: &str) -> Option<f64> {
        self.months()
            .get(month)
            .map(|models| models.values().map(|m| m.usd).sum::<Option<f64>>())
            .unwrap_or(Some(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_per_month_and_model() {
        let p = std::env::temp_dir().join(format!("znimok-meter-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let m = Meter::new(&p);
        let u = Usage {
            input_tokens: 500_000,
            output_tokens: 50_000,
            ..Default::default()
        };
        m.record("2026-09", "claude-sonnet-5-5", &u).unwrap();
        m.record("2026-09", "claude-sonnet-5-5", &u).unwrap();
        m.record("2026-10", "claude-opus-5-5", &u).unwrap();
        let all = m.months();
        let s = &all["2026-09"]["claude-sonnet-5-5"];
        assert_eq!(s.requests, 2);
        assert_eq!(s.usage.input_tokens, 1_000_000);
        assert!((m.month_usd("2026-09").unwrap() - 3.0).abs() < 1e-9);
        assert!((m.month_usd("2026-10").unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(m.month_usd("2027-01"), Some(0.0));
        m.record("2026-10", "claude-unknown", &u).unwrap();
        assert_eq!(m.month_usd("2026-10"), None);
        let _ = std::fs::remove_file(&p);
    }
}
