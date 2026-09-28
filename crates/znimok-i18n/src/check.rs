//! The rules for language files. English is the reference: its comments carry the instructions
//! (`@where`, `@kind`, `@max`, `@note`), every other file must have the same ids, variables and
//! comments, the plural categories of its language, and fit `@max` with typical values.
//! Errors make a language incomplete (it is not offered in the menu); warnings are style hints.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use fluent_syntax::ast::{
    Entry, Expression, InlineExpression, Pattern, PatternElement, VariantKey,
};
use intl_pluralrules::{PluralCategory, PluralRuleType, PluralRules};
use unic_langid::LanguageIdentifier;

/// Allowed values of `@kind`.
pub const KINDS: &[&str] = &[
    "button",
    "menu",
    "tab",
    "option",
    "label",
    "heading",
    "title",
    "hint",
    "body",
    "tooltip",
    "a11y",
    "placeholder",
    "status",
    "toast",
    "error",
    "badge",
    "value",
];

#[derive(Clone, Debug)]
pub struct Problem {
    pub lang: String,
    pub id: Option<String>,
    pub error: bool,
    pub text: String,
}

#[derive(Default, Debug)]
pub struct Report {
    pub problems: Vec<Problem>,
    /// Messages per language.
    pub counts: BTreeMap<String, usize>,
}

impl Report {
    fn err(&mut self, lang: &str, id: Option<&str>, text: impl Into<String>) {
        self.problems.push(Problem {
            lang: lang.into(),
            id: id.map(Into::into),
            error: true,
            text: text.into(),
        });
    }
    fn warn(&mut self, lang: &str, id: Option<&str>, text: impl Into<String>) {
        self.problems.push(Problem {
            lang: lang.into(),
            id: id.map(Into::into),
            error: false,
            text: text.into(),
        });
    }

    pub fn errors(&self) -> usize {
        self.problems.iter().filter(|p| p.error).count()
    }

    /// No errors for this language (warnings allowed).
    pub fn is_complete(&self, lang: &str) -> bool {
        self.counts.contains_key(lang) && !self.problems.iter().any(|p| p.error && p.lang == lang)
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for p in &self.problems {
            let _ = writeln!(
                s,
                "{} {}{}: {}",
                if p.error {
                    "ПОМИЛКА"
                } else {
                    "увага "
                },
                p.lang,
                p.id.as_deref().map(|i| format!(" {i}")).unwrap_or_default(),
                p.text
            );
        }
        let _ = write!(
            s,
            "повідомлень: {:?}; помилок: {}",
            self.counts,
            self.errors()
        );
        s
    }
}

/// One message as the checks see it.
#[derive(Debug)]
pub(crate) struct Msg {
    pub comment: Vec<String>,
    pub vars: BTreeSet<String>,
    /// Plural select: (selector variable, variant keys).
    pub plural: Option<(String, Vec<String>)>,
    /// Every variant rendered with typical values.
    pub samples: Vec<String>,
}

/// Plural categories a language uses for cardinal numbers (probed, the crate has no list).
pub fn plural_categories(lang: &str) -> Vec<&'static str> {
    let id: LanguageIdentifier = lang.parse().unwrap_or_default();
    let Ok(pr) = PluralRules::create(id, PluralRuleType::CARDINAL) else {
        return vec!["one", "other"];
    };
    let mut seen: Vec<PluralCategory> = Vec::new();
    let mut add = |c: PluralCategory| {
        if !seen.contains(&c) {
            seen.push(c);
        }
    };
    for n in 0..=200u32 {
        if let Ok(c) = pr.select(n) {
            add(c);
        }
    }
    for f in ["0.5", "1.5", "2.5"] {
        if let Ok(c) = pr.select(f) {
            add(c);
        }
    }
    let order = [
        (PluralCategory::ZERO, "zero"),
        (PluralCategory::ONE, "one"),
        (PluralCategory::TWO, "two"),
        (PluralCategory::FEW, "few"),
        (PluralCategory::MANY, "many"),
        (PluralCategory::OTHER, "other"),
    ];
    order
        .iter()
        .filter(|(c, _)| seen.contains(c))
        .map(|(_, n)| *n)
        .collect()
}

const CLDR: &[&str] = &["zero", "one", "two", "few", "many", "other"];

/// A typical value for a variable, by its name — what `@max` is measured with.
pub fn sample(var: &str) -> &'static str {
    match var {
        "count" | "n" | "index" | "total" => "12",
        "width" | "height" | "cw" | "ch" => "1920",
        "time" => "00:12",
        "date" => "2026-09-28",
        "seconds" => "4.9",
        "size" => "1.2 MB",
        "version" => "1.0.3",
        "nits" => "200",
        "fps" => "60",
        "price" => "$0.01",
        "key" | "combo" | "region" | "screen" | "record" => "Alt+Shift+4",
        "format" => "PNG",
        _ => "Xxxxxxxxxx",
    }
}

fn inline_vars(e: &InlineExpression<&str>, out: &mut BTreeSet<String>) {
    match e {
        InlineExpression::VariableReference { id } => {
            out.insert(id.name.to_string());
        }
        InlineExpression::Placeable { expression } => expr_vars(expression, out),
        InlineExpression::FunctionReference { arguments, .. } => {
            for a in &arguments.positional {
                inline_vars(a, out);
            }
        }
        _ => {}
    }
}

fn expr_vars(e: &Expression<&str>, out: &mut BTreeSet<String>) {
    match e {
        Expression::Inline(i) => inline_vars(i, out),
        Expression::Select { selector, variants } => {
            inline_vars(selector, out);
            for v in variants {
                pattern_vars(&v.value, out);
            }
        }
    }
}

pub(crate) fn pattern_vars(p: &Pattern<&str>, out: &mut BTreeSet<String>) {
    for el in &p.elements {
        if let PatternElement::Placeable { expression } = el {
            expr_vars(expression, out);
        }
    }
}

fn render_inline(e: &InlineExpression<&str>) -> Vec<String> {
    match e {
        InlineExpression::StringLiteral { value } => {
            vec![value.replace("\\\"", "\"").replace("\\\\", "\\")]
        }
        InlineExpression::NumberLiteral { value } => vec![value.to_string()],
        InlineExpression::VariableReference { id } => vec![sample(id.name).to_string()],
        InlineExpression::Placeable { expression } => render_expr(expression),
        InlineExpression::MessageReference { id, .. }
        | InlineExpression::TermReference { id, .. } => {
            vec![id.name.to_string()]
        }
        InlineExpression::FunctionReference { arguments, .. } => arguments
            .positional
            .first()
            .map(render_inline)
            .unwrap_or_else(|| vec![String::new()]),
    }
}

fn render_expr(e: &Expression<&str>) -> Vec<String> {
    match e {
        Expression::Inline(i) => render_inline(i),
        Expression::Select { variants, .. } => {
            variants.iter().flat_map(|v| render(&v.value)).collect()
        }
    }
}

/// Every alternative text of a pattern (a select multiplies), variables filled with [`sample`].
pub(crate) fn render(p: &Pattern<&str>) -> Vec<String> {
    let mut acc = vec![String::new()];
    for el in &p.elements {
        let alts = match el {
            PatternElement::TextElement { value } => vec![value.to_string()],
            PatternElement::Placeable { expression } => render_expr(expression),
        };
        acc = acc
            .iter()
            .flat_map(|a| alts.iter().map(move |b| format!("{a}{b}")))
            .collect();
    }
    acc
}

fn plural_of(p: &Pattern<&str>) -> Option<(String, Vec<String>)> {
    p.elements.iter().find_map(|el| match el {
        PatternElement::Placeable {
            expression:
                Expression::Select {
                    selector: InlineExpression::VariableReference { id },
                    variants,
                },
        } => {
            let keys: Vec<String> = variants
                .iter()
                .filter_map(|v| match &v.key {
                    VariantKey::Identifier { name } => Some(name.to_string()),
                    VariantKey::NumberLiteral { .. } => None,
                })
                .collect();
            keys.iter()
                .all(|k| CLDR.contains(&k.as_str()))
                .then(|| (id.name.to_string(), keys))
        }
        _ => None,
    })
}

/// Parse one file into (id → message, id order); parse errors and duplicates go to the report.
pub(crate) fn parse(
    lang: &str,
    src: &str,
    report: &mut Report,
) -> (BTreeMap<String, Msg>, Vec<String>, bool) {
    let res = match fluent_syntax::parser::parse(src) {
        Ok(r) => r,
        Err((r, errs)) => {
            for e in errs {
                report.err(
                    lang,
                    None,
                    format!("синтаксис Fluent: {:?} біля байта {}", e.kind, e.pos.start),
                );
            }
            r
        }
    };
    let mut map = BTreeMap::new();
    let mut order = Vec::new();
    let mut has_header = false;
    for entry in &res.body {
        match entry {
            Entry::ResourceComment(_) => has_header = true,
            Entry::Junk { content } => report.err(
                lang,
                None,
                format!(
                    "нерозібраний фрагмент: {}",
                    content.lines().next().unwrap_or("")
                ),
            ),
            Entry::Message(m) => {
                let id = m.id.name.to_string();
                let Some(value) = &m.value else {
                    report.err(lang, Some(&id), "повідомлення без тексту");
                    continue;
                };
                let mut vars = BTreeSet::new();
                pattern_vars(value, &mut vars);
                let msg = Msg {
                    comment: m
                        .comment
                        .as_ref()
                        .map(|c| c.content.iter().map(|l| l.to_string()).collect())
                        .unwrap_or_default(),
                    vars,
                    plural: plural_of(value),
                    samples: render(value),
                };
                if map.insert(id.clone(), msg).is_some() {
                    report.err(lang, Some(&id), "id повторюється");
                } else {
                    order.push(id);
                }
            }
            _ => {}
        }
    }
    (map, order, has_header)
}

fn tag<'a>(comment: &'a [String], name: &str) -> Option<&'a str> {
    let prefix = format!("@{name}:");
    comment
        .iter()
        .find_map(|l| l.strip_prefix(&prefix))
        .map(str::trim)
}

/// Check a set of files: `(language code, Fluent source)`; one of them must be `en`.
pub fn check_sources(files: &[(&str, &str)]) -> Report {
    let mut report = Report::default();
    let Some((_, en_src)) = files.iter().find(|(l, _)| *l == "en") else {
        report.err("en", None, "немає en.ftl — англійська є еталоном");
        return report;
    };
    let (en, _, en_header) = parse("en", en_src, &mut report);
    if !en_header {
        report.err("en", None, "немає заголовка ### з правилами й глосарієм");
    }
    // The reference comments.
    for (id, m) in &en {
        match tag(&m.comment, "where") {
            Some(w) if !w.is_empty() => {}
            _ => report.err("en", Some(id), "немає @where — де показується текст"),
        }
        match tag(&m.comment, "kind") {
            Some(k) if KINDS.contains(&k) => {}
            Some(k) => report.err("en", Some(id), format!("невідомий @kind «{k}»")),
            None => report.err("en", Some(id), "немає @kind"),
        }
        if let Some(mx) = tag(&m.comment, "max")
            && mx.parse::<usize>().is_err()
        {
            report.err("en", Some(id), format!("@max не число: {mx}"));
        }
    }
    for (lang, src) in files {
        if crate::is_banned(lang) {
            report.err(
                lang,
                None,
                "російська мова заборонена назавжди (рішення власника 29.09.2026)",
            );
            continue;
        }
        let (msgs, _, header) = if *lang == "en" {
            // Parsed above; parse again without re-reporting syntax errors.
            parse(lang, src, &mut Report::default())
        } else {
            parse(lang, src, &mut report)
        };
        report.counts.insert(lang.to_string(), msgs.len());
        if *lang != "en" && !header {
            report.err(lang, None, "немає заголовка ### (скопіюйте з en.ftl)");
        }
        let cats = plural_categories(lang);
        for (id, e) in &en {
            let Some(m) = msgs.get(id) else {
                if *lang != "en" {
                    report.err(lang, Some(id), "бракує перекладу");
                }
                continue;
            };
            if *lang != "en" && m.comment != e.comment {
                report.err(
                    lang,
                    Some(id),
                    "коментар відрізняється від en.ftl (виправляє `znimok-i18n sync-comments`)",
                );
            }
            if m.vars != e.vars {
                report.err(
                    lang,
                    Some(id),
                    format!("змінні {:?}, а в en {:?}", m.vars, e.vars),
                );
            }
            match (&e.plural, &m.plural) {
                (Some(_), None) => report.err(lang, Some(id), "в en множина, тут ні"),
                (_, Some((_, keys))) => {
                    for c in &cats {
                        if !keys.iter().any(|k| k == c) {
                            report.err(lang, Some(id), format!("бракує форми множини [{c}]"));
                        }
                    }
                    for k in keys {
                        if !cats.contains(&k.as_str()) {
                            report.warn(
                                lang,
                                Some(id),
                                format!("форма [{k}] у цій мові не вживається"),
                            );
                        }
                    }
                }
                _ => {}
            }
            let max = tag(&e.comment, "max").and_then(|v| v.parse::<usize>().ok());
            for s in &m.samples {
                let len = s.chars().count();
                if s.trim().is_empty() {
                    report.err(lang, Some(id), "порожній текст");
                }
                if let Some(mx) = max
                    && len > mx
                {
                    report.err(lang, Some(id), format!("{len} символів > @max {mx}: «{s}»"));
                }
                if s.contains("...") {
                    report.warn(lang, Some(id), "три крапки замість «…»");
                }
                if *lang == "uk" && s.contains('"') {
                    report.warn(lang, Some(id), "прямі лапки — в українській «…»");
                }
            }
        }
        for id in msgs.keys() {
            if !en.contains_key(id) {
                report.err(lang, Some(id), "id немає в en.ftl");
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "### header\n\n# @where: somewhere\n# @kind: button\n# @max: 10\nok = OK\n\n# @where: list\n# @kind: label\nitems = { $count ->\n    [one] { $count } item\n   *[other] { $count } items\n}\n";

    #[test]
    fn categories_of_languages() {
        assert_eq!(plural_categories("en"), vec!["one", "other"]);
        assert_eq!(plural_categories("uk"), vec!["one", "few", "many", "other"]);
    }

    #[test]
    fn catches_missing_forms_vars_length_and_comments() {
        let uk = "### h\n\n# @where: somewhere\n# @kind: button\n# @max: 10\nok = Дуже-дуже довге\n\n# @where: list\n# @kind: label\nitems = { $n ->\n    [one] { $n } штука\n   *[other] { $n } штук\n}\n\nextra = зайве\n";
        let r = check_sources(&[("en", EN), ("uk", uk)]);
        let t = r.render();
        assert!(t.contains("> @max 10"), "{t}");
        assert!(t.contains("бракує форми множини [few]"), "{t}");
        assert!(t.contains("змінні"), "{t}");
        assert!(t.contains("id немає в en.ftl"), "{t}");
        assert!(!r.is_complete("uk") && r.is_complete("en"));
    }

    #[test]
    fn english_needs_where_and_kind() {
        let r = check_sources(&[("en", "### h\n# @kind: bogus\nx = X\n")]);
        let t = r.render();
        assert!(
            t.contains("немає @where") && t.contains("невідомий @kind"),
            "{t}"
        );
    }
}
