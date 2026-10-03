"""The privacy policy as pages of the site (ZK-259): docs/privacy.md → site/privacy.html,
docs/privacy.en.md → site/en/privacy.html. Google's OAuth consent screen wants the policy on the
app's own domain (v-plum.github.io), not on github.com.

    python tools/build_privacy.py          # writes the two pages
    python tools/build_privacy.py --check  # fails when a page is out of date (CI)

A small Markdown subset is enough: headings, paragraphs, lists (one level, numbered or not),
tables, quotes, **bold**, `code`, [links](…). Links to the other language's .md point to its page.
"""
import html
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PAGES = [
    ("docs/privacy.md", "site/privacy.html", "uk", "", "Приватність — Znimok", "en/privacy.html", "English"),
    ("docs/privacy.en.md", "site/en/privacy.html", "en", "../", "Privacy — Znimok", "../privacy.html", "Українська"),
]


def inline(t: str) -> str:
    t = html.escape(t, quote=False)
    t = re.sub(r"`([^`]+)`", r"<code>\1</code>", t)
    t = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", t)

    def link(m):
        href = m.group(2)
        href = {"privacy.md": "../privacy.html", "privacy.en.md": "en/privacy.html"}.get(href, href)
        return f'<a href="{href}">{m.group(1)}</a>'

    return re.sub(r"\[([^\]]+)\]\(([^)]+)\)", link, t)


def render(md: str) -> str:
    out, para, items, kind, table = [], [], [], None, []

    def flush():
        nonlocal para, items, kind, table
        if para:
            out.append("<p>" + inline(" ".join(para)) + "</p>")
        if items:
            out.append(f"<{kind}>" + "".join("<li>" + inline(i) + "</li>" for i in items) + f"</{kind}>")
        if table:
            rows = [[c.strip() for c in r.strip("|").split("|")] for r in table if not re.match(r"^\|[\s:|-]+\|$", r)]
            head, body = rows[0], rows[1:]
            out.append("<table><thead><tr>" + "".join(f"<th>{inline(c)}</th>" for c in head) + "</tr></thead><tbody>"
                       + "".join("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in r) + "</tr>" for r in body) + "</tbody></table>")
        para, items, kind, table = [], [], None, []

    for line in md.splitlines():
        s = line.rstrip()
        if not s:
            flush()
            continue
        if s.startswith("|"):
            if not table:
                flush()
            table.append(s)
            continue
        m = re.match(r"^(#{1,3}) (.*)$", s)
        if m:
            flush()
            n = len(m.group(1))
            out.append(f"<h{n}>{inline(m.group(2))}</h{n}>")
            continue
        if s.startswith("> "):
            flush()
            out.append("<blockquote>" + inline(s[2:]) + "</blockquote>")
            continue
        m = re.match(r"^(\d+\.|-) (.*)$", s)
        if m:
            k = "ol" if m.group(1)[0].isdigit() else "ul"
            if para or (kind and kind != k):
                flush()
            kind = k
            items.append(m.group(2))
            continue
        if items and line[:1].isspace():
            items[-1] += " " + s.strip()
            continue
        para.append(s.strip())
    flush()
    return "\n".join(out)


def page(src, lang, up, title, other, other_name):
    body = render((ROOT / src).read_text(encoding="utf-8"))
    return f"""<!doctype html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)}</title>
<link rel="icon" type="image/png" href="{up}assets/img/favicon.png">
<link rel="stylesheet" href="{up}assets/site.css">
<style>.doc{{max-width:760px;margin:0 auto;padding:8px 16px 48px;line-height:1.6}}.doc table{{border-collapse:collapse;width:100%;margin:12px 0}}.doc td,.doc th{{border:1px solid var(--line,#ddd);padding:6px 8px;text-align:left;vertical-align:top}}.doc blockquote{{margin:12px 0;padding:8px 14px;border-left:3px solid var(--accent,#2f6fed);opacity:.85}}.doc code{{font-size:.92em}}</style>
</head>
<body>
<div class="wrap">
  <header class="top">
    <a class="brand" href="./"><img src="{up}assets/img/icon.png" alt=""><span class="wordmark">Znimok</span></a>
    <nav><a class="lang" href="{other}">{other_name}</a></nav>
  </header>
  <main class="doc">
{body}
  </main>
</div>
</body>
</html>
"""


def main():
    check = "--check" in sys.argv
    stale = []
    for src, dst, lang, up, title, other, other_name in PAGES:
        want = page(src, lang, up, title, other, other_name)
        p = ROOT / dst
        if check:
            if not p.exists() or p.read_text(encoding="utf-8") != want:
                stale.append(dst)
        else:
            p.write_text(want, encoding="utf-8", newline="\n")
            print("wrote", dst)
    if stale:
        sys.exit("out of date: " + ", ".join(stale) + " — run python tools/build_privacy.py")


if __name__ == "__main__":
    main()
