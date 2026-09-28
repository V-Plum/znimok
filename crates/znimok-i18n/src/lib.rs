//! Znimok localisation (PLAN §5.11, ZK-29).
//!
//! The reference files are `i18n/en.ftl` and `i18n/uk.ftl` (Fluent). English and Ukrainian are
//! always complete; the interface language is the system language when a complete translation
//! exists, otherwise English; a message missing in a language falls back to English.
//!
//! - [`Localizer`] — formatting at run time (core, CLI, MCP; the UI gets `.po` files from [`po`]);
//! - [`check`] — the rules every language file must pass (run by `cargo test` in CI);
//! - [`sync`] — copies the instruction comments of `en.ftl` into the other files;
//! - [`po`] — gettext files for Slint's `@tr("id" => "English")`.

pub mod check;
pub mod po;
pub mod sync;

use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use unic_langid::LanguageIdentifier;

pub use fluent_bundle::{FluentArgs, FluentValue};

/// The language every other one falls back to.
pub const FALLBACK: &str = "en";

/// The files built into the program: (language code, Fluent source).
pub const BUILT_IN: &[(&str, &str)] = &[
    ("en", include_str!("../../../i18n/en.ftl")),
    ("uk", include_str!("../../../i18n/uk.ftl")),
];

fn source(lang: &str) -> Option<&'static str> {
    BUILT_IN.iter().find(|(l, _)| *l == lang).map(|(_, s)| *s)
}

/// Built-in languages that pass [`check`] (only these are offered in the language menu).
pub fn available() -> Vec<&'static str> {
    let report = check::check_sources(BUILT_IN);
    BUILT_IN
        .iter()
        .map(|(l, _)| *l)
        .filter(|l| report.is_complete(l))
        .collect()
}

/// The OS interface language as a BCP 47 tag (`uk-UA`, `en-US`), if the OS tells.
pub fn system_language() -> Option<String> {
    sys_locale::get_locale()
}

/// Pick the interface language: the user's explicit setting if available, else the system
/// language (by its primary subtag: `uk-UA` → `uk`) if available, else English.
pub fn choose_language(setting: Option<&str>, system: Option<&str>) -> &'static str {
    let avail = available();
    let primary = |tag: &str| {
        tag.split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    for want in [setting, system].into_iter().flatten() {
        let p = primary(want);
        if let Some(l) = avail.iter().find(|l| **l == p) {
            return l;
        }
    }
    FALLBACK
}

/// Formats messages of one language with English as the fallback. Shareable between threads.
pub struct Localizer {
    lang: &'static str,
    /// The chosen language first, then English (unless they are the same).
    bundles: Vec<FluentBundle<FluentResource>>,
}

fn bundle(lang: &'static str) -> FluentBundle<FluentResource> {
    let id: LanguageIdentifier = lang.parse().unwrap_or_default();
    let mut b = FluentBundle::new_concurrent(vec![id]);
    // No Unicode isolation marks around placeables: the UI is left-to-right and the marks would
    // show up in logs, file names and the clipboard.
    b.set_use_isolating(false);
    let res =
        FluentResource::try_new(source(lang).unwrap_or("").to_string()).unwrap_or_else(|(r, _)| r);
    let _ = b.add_resource(res);
    b
}

impl Localizer {
    /// `lang` is a code from [`BUILT_IN`]; anything else gives English.
    pub fn new(lang: &str) -> Self {
        let lang = BUILT_IN
            .iter()
            .map(|(l, _)| *l)
            .find(|l| *l == lang)
            .unwrap_or(FALLBACK);
        let mut bundles = vec![bundle(lang)];
        if lang != FALLBACK {
            bundles.push(bundle(FALLBACK));
        }
        Self { lang, bundles }
    }

    /// The language for this OS and the user's setting (see [`choose_language`]).
    pub fn for_system(setting: Option<&str>) -> Self {
        Self::new(choose_language(setting, system_language().as_deref()))
    }

    pub fn lang(&self) -> &'static str {
        self.lang
    }

    pub fn has(&self, id: &str) -> bool {
        self.bundles.iter().any(|b| b.has_message(id))
    }

    /// Message without variables. An unknown id returns the id itself (visible, never a panic).
    pub fn tr(&self, id: &str) -> String {
        self.format(id, None)
    }

    /// Message with variables: `tr_args("pill-where", &args!(width = 1240, height = 680))`.
    pub fn tr_args(&self, id: &str, args: &FluentArgs) -> String {
        self.format(id, Some(args))
    }

    fn format(&self, id: &str, args: Option<&FluentArgs>) -> String {
        for b in &self.bundles {
            if let Some(p) = b.get_message(id).and_then(|m| m.value()) {
                let mut errors = Vec::new();
                return b.format_pattern(p, args, &mut errors).into_owned();
            }
        }
        id.to_string()
    }
}

/// Build [`FluentArgs`]: `args!(count = 3, name = "Знімок")`.
#[macro_export]
macro_rules! args {
    ($($k:ident = $v:expr),* $(,)?) => {{
        let mut a = $crate::FluentArgs::new();
        $( a.set(stringify!($k), $v); )*
        a
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_languages_pass_all_checks() {
        let r = check::check_sources(BUILT_IN);
        assert!(r.problems.iter().all(|p| !p.error), "{}", r.render());
        assert_eq!(available(), vec!["en", "uk"]);
    }

    #[test]
    fn plurals_follow_each_language() {
        let uk = Localizer::new("uk");
        let en = Localizer::new("en");
        let n = |l: &Localizer, c: i64| l.tr_args("common-marks", &args!(count = c));
        assert_eq!(n(&uk, 1), "1 позначка");
        assert_eq!(n(&uk, 3), "3 позначки");
        assert_eq!(n(&uk, 5), "5 позначок");
        assert_eq!(n(&uk, 21), "21 позначка");
        assert_eq!(n(&uk, 11), "11 позначок");
        assert_eq!(n(&en, 1), "1 annotation");
        assert_eq!(n(&en, 5), "5 annotations");
    }

    #[test]
    fn variables_have_no_isolation_marks() {
        let uk = Localizer::new("uk");
        let s = uk.tr_args("pill-where", &args!(width = 1240, height = 680));
        assert_eq!(s, "1240 × 680 · у буфері й бібліотеці");
        assert!(!s.contains('\u{2068}'));
    }

    #[test]
    fn language_choice_and_fallback() {
        assert_eq!(choose_language(None, Some("uk-UA")), "uk");
        assert_eq!(choose_language(None, Some("de-DE")), "en");
        assert_eq!(choose_language(Some("en"), Some("uk-UA")), "en");
        assert_eq!(choose_language(Some("fr"), Some("uk_UA")), "uk");
        assert_eq!(Localizer::new("pl").lang(), "en");
        let uk = Localizer::new("uk");
        assert_eq!(uk.tr("doc-copy"), "Копіювати");
        assert_eq!(uk.tr("no-such-id"), "no-such-id");
        assert_eq!(uk.tr("look-language-name"), "Українська");
    }
}
