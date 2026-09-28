//! Keep every language file in the shape of `en.ftl`: the same header (except its first line,
//! which names the language), the same groups and order, and the same instruction comment above
//! each message. Only the message values are the translator's. Running it on `en.ftl` itself
//! normalises the formatting.

use fluent_syntax::ast::{Comment, Entry, Message, Resource};
use fluent_syntax::serializer;

fn one(entry: Entry<&str>) -> String {
    serializer::serialize(&Resource { body: vec![entry] })
}

/// Rewrite `target` (a language file) after `en`. Messages missing in `target` are left out
/// (the check reports them); messages not in English go to the end under their own group.
pub fn sync_comments(en: &str, target: &str) -> Result<String, String> {
    // Git on Windows checks the files out with CRLF; the output is always LF.
    let (en, target) = (en.replace("\r\n", "\n"), target.replace("\r\n", "\n"));
    let (en, target) = (en.as_str(), target.as_str());
    let en_res = fluent_syntax::parser::parse(en)
        .map_err(|(_, e)| format!("en.ftl: {} помилок синтаксису", e.len()))?;
    let tg_res = fluent_syntax::parser::parse(target)
        .map_err(|(_, e)| format!("файл: {} помилок синтаксису", e.len()))?;
    let mut theirs: Vec<Message<&str>> = tg_res
        .body
        .into_iter()
        .filter_map(|e| match e {
            Entry::Message(m) => Some(m),
            _ => None,
        })
        .collect();
    // The target's own first header line (it names the language).
    let own_title = fluent_syntax::parser::parse(target).ok().and_then(|r| {
        r.body.into_iter().find_map(|e| match e {
            Entry::ResourceComment(c) => c.content.first().map(|s| s.to_string()),
            _ => None,
        })
    });
    let mut out = String::new();
    let mut header_done = false;
    for entry in en_res.body {
        match entry {
            Entry::ResourceComment(c) if !header_done => {
                header_done = true;
                let mut lines: Vec<String> = c.content.iter().map(|s| s.to_string()).collect();
                if let (Some(t), Some(first)) = (&own_title, lines.first_mut()) {
                    *first = t.clone();
                }
                let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
                out.push_str(&one(Entry::ResourceComment(Comment { content: refs })));
            }
            Entry::GroupComment(c) => {
                out.push('\n');
                out.push_str(&one(Entry::GroupComment(c)));
            }
            Entry::Message(m) => {
                if let Some(pos) = theirs.iter().position(|t| t.id.name == m.id.name) {
                    let mut t = theirs.remove(pos);
                    t.comment = m.comment;
                    out.push('\n');
                    out.push_str(&one(Entry::Message(t)));
                }
            }
            other => {
                out.push('\n');
                out.push_str(&one(other));
            }
        }
    }
    if !theirs.is_empty() {
        out.push_str("\n## Not in en.ftl — remove or add to English\n");
        for t in theirs {
            out.push('\n');
            out.push_str(&one(Entry::Message(t)));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_files_are_already_in_sync() {
        let lf = |s: &str| s.replace("\r\n", "\n");
        let (en, uk) = (&lf(crate::BUILT_IN[0].1), &lf(crate::BUILT_IN[1].1));
        assert_eq!(
            sync_comments(en, en).unwrap(),
            *en,
            "en.ftl не в каноничному вигляді — `znimok-i18n sync-comments`"
        );
        assert_eq!(
            sync_comments(en, uk).unwrap(),
            *uk,
            "uk.ftl не синхронний з en.ftl — `znimok-i18n sync-comments`"
        );
    }

    #[test]
    fn copies_comments_and_order() {
        let en = "### Znimok — English\n### rules\n\n## G\n\n# @where: a\n# @kind: button\na = A\n\n# @where: b\n# @kind: label\nb = B\n";
        let xx = "### Znimok — Klingon\n\nb = bB\n# old\na = aA\nz = Z\n";
        let out = sync_comments(en, xx).unwrap();
        assert!(out.starts_with("### Znimok — Klingon\n### rules\n"));
        let a = out.find("a = aA").unwrap();
        let b = out.find("b = bB").unwrap();
        assert!(
            a < b && out.contains("# @where: a\n# @kind: button\na = aA") && !out.contains("# old")
        );
        assert!(out.contains("## Not in en.ftl") && out.contains("z = Z"));
    }
}
