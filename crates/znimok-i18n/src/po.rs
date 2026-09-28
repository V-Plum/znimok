//! Gettext `.po` for Slint. The `.slint` code writes `@tr("id" => "English text {0}", arg)`:
//! the context is the Fluent id and the source string is the English message, so English needs
//! no `.po` at all and every other language gets `msgctxt id / msgid English / msgstr translation`.
//! Variables become Slint placeholders: the plural selector is `{n}`, the others `{0}`, `{1}`…
//! in the order they first appear in the English message. Literal braces become `{{` `}}`.
//! [`slint_refs`] prints the exact `@tr(…)` for every id.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use fluent_syntax::ast::{
    Entry, Expression, InlineExpression, Pattern, PatternElement, VariantKey,
};

/// `Plural-Forms` header and the order of CLDR categories in `msgstr[i]` for a language.
pub fn plural_forms(lang: &str) -> Option<(&'static str, &'static [&'static str])> {
    match lang {
        "en" | "de" | "nl" | "sv" | "da" | "no" | "nb" | "it" | "es" | "pt" | "fi" | "et"
        | "el" | "hu" | "bg" => Some(("nplurals=2; plural=(n != 1);", &["one", "other"])),
        "uk" | "be" => Some((
            "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
            &["one", "few", "many"],
        )),
        "pl" => Some((
            "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
            &["one", "few", "many"],
        )),
        "cs" | "sk" => Some((
            "nplurals=3; plural=(n==1) ? 0 : (n>=2 && n<=4) ? 1 : 2;",
            &["one", "few", "other"],
        )),
        "fr" => Some(("nplurals=2; plural=(n > 1);", &["one", "other"])),
        "ja" | "zh" | "ko" | "vi" | "th" => Some(("nplurals=1; plural=0;", &["other"])),
        _ => None,
    }
}

/// Chooses the variant of a plural select by its keys.
type Pick = dyn Fn(&[(String, &Pattern<&str>)]) -> Option<usize>;

struct Placeholders {
    selector: Option<String>,
    order: Vec<String>,
}

impl Placeholders {
    fn of(en: &Pattern<&str>) -> Self {
        let mut vars = std::collections::BTreeSet::new();
        crate::check::pattern_vars(en, &mut vars);
        let selector = en.elements.iter().find_map(|el| match el {
            PatternElement::Placeable {
                expression:
                    Expression::Select {
                        selector: InlineExpression::VariableReference { id },
                        ..
                    },
            } => Some(id.name.to_string()),
            _ => None,
        });
        let mut order = Vec::new();
        collect_order(en, &mut order);
        order.retain(|v| Some(v) != selector.as_ref());
        Self { selector, order }
    }

    fn slot(&self, var: &str) -> String {
        if Some(var) == self.selector.as_deref() {
            "{n}".into()
        } else {
            match self.order.iter().position(|v| v == var) {
                Some(i) => format!("{{{i}}}"),
                None => format!("{{{var}}}"),
            }
        }
    }
}

fn collect_order(p: &Pattern<&str>, out: &mut Vec<String>) {
    for el in &p.elements {
        if let PatternElement::Placeable { expression } = el {
            match expression {
                Expression::Inline(InlineExpression::VariableReference { id }) => {
                    if !out.iter().any(|v| v == id.name) {
                        out.push(id.name.to_string());
                    }
                }
                Expression::Select { selector, variants } => {
                    if let InlineExpression::VariableReference { id } = selector
                        && !out.iter().any(|v| v == id.name)
                    {
                        out.push(id.name.to_string());
                    }
                    for v in variants {
                        collect_order(&v.value, out);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Slint text of a pattern; `pick` chooses the variant of a plural select.
fn slint_text(p: &Pattern<&str>, ph: &Placeholders, pick: &Pick) -> String {
    let mut s = String::new();
    for el in &p.elements {
        match el {
            PatternElement::TextElement { value } => {
                s.push_str(&value.replace('{', "{{").replace('}', "}}"))
            }
            PatternElement::Placeable { expression } => match expression {
                Expression::Inline(InlineExpression::VariableReference { id }) => {
                    s.push_str(&ph.slot(id.name))
                }
                Expression::Inline(InlineExpression::StringLiteral { value }) => {
                    s.push_str(&value.replace('{', "{{").replace('}', "}}"))
                }
                Expression::Inline(InlineExpression::NumberLiteral { value }) => s.push_str(value),
                Expression::Select { variants, .. } => {
                    let vs: Vec<(String, &Pattern<&str>)> = variants
                        .iter()
                        .map(|v| {
                            let k = match &v.key {
                                VariantKey::Identifier { name } => name.to_string(),
                                VariantKey::NumberLiteral { value } => value.to_string(),
                            };
                            (k, &v.value)
                        })
                        .collect();
                    let i = pick(&vs)
                        .or_else(|| variants.iter().position(|v| v.default))
                        .unwrap_or(0);
                    s.push_str(&slint_text(vs[i].1, ph, pick));
                }
                _ => {}
            },
        }
    }
    s
}

fn by_key<'a>(key: &'a str) -> impl Fn(&[(String, &Pattern<&str>)]) -> Option<usize> + 'a {
    move |vs| vs.iter().position(|(k, _)| k == key)
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn messages(src: &str) -> Result<BTreeMap<&str, (usize, Pattern<&str>)>, String> {
    let res = fluent_syntax::parser::parse(src)
        .map_err(|(_, e)| format!("{} помилок синтаксису", e.len()))?;
    Ok(res
        .body
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Entry::Message(m) => m.value.map(|v| (m.id.name, (i, v))),
            _ => None,
        })
        .collect())
}

/// A banned language that got into a build: every English text, every word → 💩.
fn spoiled_po(en: &str, lang: &str) -> String {
    let mut out = format!(
        "# {lang}: banned language (owner, 29.09.2026)\nmsgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\"Language: {lang}\\n\"\n\"Plural-Forms: nplurals=1; plural=0;\\n\"\n"
    );
    let Ok(en_m) = messages(en) else {
        return out;
    };
    let mut ids: Vec<(&str, usize)> = en_m.iter().map(|(k, (i, _))| (*k, *i)).collect();
    ids.sort_by_key(|(_, i)| *i);
    for (id, _) in ids {
        let e = &en_m[id].1;
        let ph = Placeholders::of(e);
        let never = |_: &[(String, &Pattern<&str>)]| None;
        let text = slint_text(e, &ph, &never);
        let _ = write!(
            out,
            "\nmsgctxt \"{}\"\nmsgid \"{}\"\nmsgstr \"{}\"\n",
            esc(id),
            esc(&text),
            esc(&crate::spoil(&text))
        );
    }
    out
}

/// The `.po` of `lang` from `en` and that language's file.
pub fn to_po(en: &str, target: &str, lang: &str) -> Result<String, String> {
    if crate::is_banned(lang) {
        return Ok(spoiled_po(en, lang));
    }
    let (forms, cats) =
        plural_forms(lang).ok_or(format!("Plural-Forms для «{lang}» не задано в po.rs"))?;
    let en_m = messages(en)?;
    let tg_m = messages(target)?;
    let mut ids: Vec<(&str, usize)> = en_m.iter().map(|(k, (i, _))| (*k, *i)).collect();
    ids.sort_by_key(|(_, i)| *i);
    let mut out = String::new();
    let _ = write!(
        out,
        "# Generated by znimok-i18n from i18n/en.ftl and i18n/{lang}.ftl — do not edit.\nmsgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\"Language: {lang}\\n\"\n\"Plural-Forms: {forms}\\n\"\n"
    );
    for (id, _) in ids {
        let e = &en_m[id].1;
        let Some((_, t)) = tg_m.get(id) else { continue };
        let ph = Placeholders::of(e);
        let _ = write!(out, "\nmsgctxt \"{}\"\n", esc(id));
        if ph.selector.is_some() {
            let one = slint_text(e, &ph, &by_key("one"));
            let other = slint_text(e, &ph, &by_key("other"));
            let _ = writeln!(
                out,
                "msgid \"{}\"\nmsgid_plural \"{}\"",
                esc(&one),
                esc(&other)
            );
            for (i, c) in cats.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "msgstr[{i}] \"{}\"",
                    esc(&slint_text(t, &ph, &by_key(c)))
                );
            }
        } else {
            let never = |_: &[(String, &Pattern<&str>)]| None;
            let _ = writeln!(
                out,
                "msgid \"{}\"\nmsgstr \"{}\"",
                esc(&slint_text(e, &ph, &never)),
                esc(&slint_text(t, &ph, &never))
            );
        }
    }
    Ok(out)
}

/// For UI developers: the exact `@tr(…)` of every message, in file order.
pub fn slint_refs(en: &str) -> Result<String, String> {
    let en_m = messages(en)?;
    let mut ids: Vec<(&str, usize)> = en_m.iter().map(|(k, (i, _))| (*k, *i)).collect();
    ids.sort_by_key(|(_, i)| *i);
    let mut out = String::new();
    for (id, _) in ids {
        let e = &en_m[id].1;
        let ph = Placeholders::of(e);
        let args = ph.order.join(", ");
        let sep = if args.is_empty() { "" } else { ", " };
        if let Some(sel) = &ph.selector {
            let one = slint_text(e, &ph, &by_key("one"));
            let other = slint_text(e, &ph, &by_key("other"));
            let _ = writeln!(
                out,
                "@tr(\"{id}\" => \"{}\" | \"{}\" % {sel}{sep}{args})",
                esc(&one),
                esc(&other)
            );
        } else {
            let never = |_: &[(String, &Pattern<&str>)]| None;
            let _ = writeln!(
                out,
                "@tr(\"{id}\" => \"{}\"{sep}{args})",
                esc(&slint_text(e, &ph, &never))
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_and_placeholders() {
        let en = "### h\n\n# c\na = { $count ->\n    [one] { $count } mark on { $name }\n   *[other] { $count } marks on { $name }\n}\n\n# c\nb = Size { $width } × { $height } {\"{\"}date{\"}\"}\n";
        let uk = "### h\n\n# c\na = { $count ->\n    [one] { $count } позначка на { $name }\n    [few] { $count } позначки на { $name }\n    [many] { $count } позначок на { $name }\n   *[other] { $count } позначки на { $name }\n}\n\n# c\nb = Розмір { $width } × { $height } {\"{\"}date{\"}\"}\n";
        let po = to_po(en, uk, "uk").unwrap();
        assert!(po.contains("msgctxt \"a\"\nmsgid \"{n} mark on {0}\"\nmsgid_plural \"{n} marks on {0}\"\nmsgstr[0] \"{n} позначка на {0}\"\nmsgstr[1] \"{n} позначки на {0}\"\nmsgstr[2] \"{n} позначок на {0}\""), "{po}");
        assert!(
            po.contains("msgid \"Size {0} × {1} {{date}}\"\nmsgstr \"Розмір {0} × {1} {{date}}\""),
            "{po}"
        );
        let refs = slint_refs(en).unwrap();
        assert!(
            refs.contains("@tr(\"a\" => \"{n} mark on {0}\" | \"{n} marks on {0}\" % count, name)"),
            "{refs}"
        );
        assert!(
            refs.contains("@tr(\"b\" => \"Size {0} × {1} {{date}}\", width, height)"),
            "{refs}"
        );
    }

    #[test]
    fn built_in_uk_po() {
        let po = to_po(crate::BUILT_IN[0].1, crate::BUILT_IN[1].1, "uk").unwrap();
        assert!(po.contains("msgctxt \"doc-copy\"\nmsgid \"Copy\"\nmsgstr \"Копіювати\""));
        assert!(po.matches("msgctxt").count() >= 690);
    }
}
