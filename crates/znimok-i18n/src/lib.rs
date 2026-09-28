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
//!
//! **Russian — never** (owner, 29.09.2026). No Russian translation is shipped, accepted or loaded:
//! [`check`] fails on a Russian file (so CI rejects it), [`available`] never lists it, and should
//! one still get into a build, every text of it is replaced with [`BANNED_TEXT`]. A Russian OS
//! gets the English interface, like any language without a translation.

pub mod check;
pub mod po;
pub mod sync;

use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use unic_langid::LanguageIdentifier;

pub use fluent_bundle::{FluentArgs, FluentValue};

/// The language every other one falls back to.
pub const FALLBACK: &str = "en";

/// Languages Znimok never ships or loads, by primary subtag (owner, 29.09.2026).
pub const BANNED: &[&str] = &["ru"];

/// What a banned language shows instead of every word.
pub const BANNED_TEXT: &str = "💩";

/// The primary subtag, lower case: `ru-RU` → `ru`, `uk_UA` → `uk`.
fn primary(tag: &str) -> String {
    tag.trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub fn is_banned(tag: &str) -> bool {
    BANNED.contains(&primary(tag).as_str())
}

/// Every word of `text` becomes [`BANNED_TEXT`].
pub fn spoil(text: &str) -> String {
    let words = text.split_whitespace().count().max(1);
    vec![BANNED_TEXT; words].join(" ")
}

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
        .filter(|l| report.is_complete(l) && !is_banned(l))
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
    /// A banned language got in: every text is [`spoil`]ed.
    spoiled: bool,
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
    /// `lang` is a code from [`BUILT_IN`]; anything else gives English. A banned language —
    /// asked for by name or found built in — gives [`BANNED_TEXT`] for every word.
    pub fn new(lang: &str) -> Self {
        if is_banned(lang) {
            return Self {
                lang: FALLBACK,
                spoiled: true,
                bundles: vec![bundle(FALLBACK)],
            };
        }
        let lang = BUILT_IN
            .iter()
            .map(|(l, _)| *l)
            .find(|l| *l == lang)
            .unwrap_or(FALLBACK);
        let mut bundles = vec![bundle(lang)];
        if lang != FALLBACK {
            bundles.push(bundle(FALLBACK));
        }
        Self {
            lang,
            spoiled: false,
            bundles,
        }
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
        let text = self
            .bundles
            .iter()
            .find_map(|b| {
                let p = b.get_message(id).and_then(|m| m.value())?;
                let mut errors = Vec::new();
                Some(b.format_pattern(p, args, &mut errors).into_owned())
            })
            .unwrap_or_else(|| id.to_string());
        if self.spoiled { spoil(&text) } else { text }
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

    /// Owner, 29.09.2026: Russian — never. Not offered, not chosen for a Russian OS, spoiled if
    /// asked for, rejected by the checker, and no such file in the repository.
    #[test]
    fn russian_never() {
        assert!(is_banned("ru") && is_banned("ru-RU") && is_banned("RU_ua"));
        assert!(!is_banned("uk") && !is_banned("rue") && !is_banned("en"));
        assert!(!available().iter().any(|l| is_banned(l)));
        assert_eq!(choose_language(None, Some("ru-RU")), "en");
        assert_eq!(choose_language(Some("ru"), Some("uk-UA")), "uk");
        let ru = Localizer::new("ru-RU");
        assert_eq!(
            ru.tr("crash-show-folder"),
            spoil(&Localizer::new("en").tr("crash-show-folder"))
        );
        assert!(
            ru.tr("crash-show-folder")
                .chars()
                .all(|c| c == '💩' || c == ' ')
        );
        let r = check::check_sources(&[BUILT_IN[0], ("ru", BUILT_IN[0].1)]);
        assert!(
            r.problems.iter().any(|p| p.error && p.lang == "ru"),
            "{}",
            r.render()
        );
        assert!(
            po::to_po(BUILT_IN[0].1, BUILT_IN[0].1, "ru")
                .unwrap()
                .contains("msgstr \"💩")
        );
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../i18n");
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name.split('.').next().unwrap_or("");
            assert!(!is_banned(stem), "i18n/{name}: Russian is never accepted");
        }
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
