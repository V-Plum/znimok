# Znimok interface languages

*Українською коротко:* тут лежать рядки інтерфейсу Znimok у форматі [Fluent](https://projectfluent.org/).
`en.ftl` і `uk.ftl` — два повні еталони. Нова мова — ще один файл `xx.ftl` з тими самими id; його можна
доручити ШІ за інструкцією нижче. Перевірка — `cargo run -p znimok-i18n -- check`.

## Files

| File | What it is |
|---|---|
| `en.ftl` | English — the reference. Its comments are the instructions for translators. |
| `uk.ftl` | Ukrainian — the second complete reference. |
| `xx.ftl` | Any further language (ISO 639-1 code as the file name: `de.ftl`, `pl.ftl`…). |

Every message has an instruction comment, identical in all files:

```
# @where: Pill title after a region capture        ← where the text is shown
# @kind: title                                     ← what it is (button, tooltip, error…)
# @max: 40                                         ← max characters with typical values
# @note: …                                         ← anything else the translator must know (optional)
pill-region-copied = Region screenshot copied
```

The header of each file (`###` lines) holds the **rules** (tone, capitalisation, quotes, ellipsis,
key names, names that are never translated, variables, plurals) and the **glossary** of Znimok terms.

## How the program uses them

- The interface language is the system language if a **complete** translation exists, otherwise
  English. The user can pick another one in Settings → Appearance and language; the menu lists only
  complete languages.
- A message missing in a language falls back to English.
- Core, CLI and MCP read the `.ftl` files directly (`znimok-i18n` crate, `fluent-rs`).
- The Slint UI gets gettext files generated from them: `.slint` code uses
  `@tr("id" => "English text")`, the generator writes `msgctxt id / msgid English / msgstr translation`.

## Adding a language with an AI

Give the AI this file, `en.ftl`, `uk.ftl`, and the prompt below (replace `German` / `de`):

> Translate the Znimok interface into German. Create `i18n/de.ftl`.
> 1. Copy `en.ftl` completely: header, groups, comments and message ids stay exactly as they are.
>    Change only the first header line to name the language (`### Znimok — interface strings, German (de).`)
>    and the text after `=` of every message.
> 2. Read the rules and the glossary in the header first. Add a German column to the glossary lines.
> 3. For every message read `@where`, `@kind`, `@max` and `@note` before translating. Use `uk.ftl`
>    as a second opinion where the English is ambiguous.
> 4. Keep every `{ $variable }` of the English message. Never translate names listed in the rules.
> 5. Plurals: write every CLDR category of German (`one`, `other`) and mark `*[other]` as default.
> 6. Stay within `@max` characters (variables count with typical values: numbers 2–4 digits,
>    names ~10 characters). Shorten the wording rather than abbreviating.
> 7. Run `cargo run -p znimok-i18n -- check` and fix every ПОМИЛКА (error) line; warnings are style hints.

Then run:

```
cargo run -p znimok-i18n -- sync-comments   # makes comments, order and formatting identical to en.ftl
cargo run -p znimok-i18n -- check           # must print 0 errors
```

If the language needs plural forms that `crates/znimok-i18n/src/po.rs` does not know yet, add its
`Plural-Forms` line there (the check tells you).

## Changing or adding a message

1. Edit `en.ftl` (text and the instruction comment), then `uk.ftl` (text only).
2. `cargo run -p znimok-i18n -- sync-comments` copies the comment to every file.
3. `cargo run -p znimok-i18n -- check` — languages that now lack the message become incomplete
   until they get it (the program shows English for it meanwhile).

`cargo test` runs the same check, so CI fails on an incomplete English or Ukrainian file.

## Commands

```
cargo run -p znimok-i18n -- check [DIR]          # all *.ftl against the rules
cargo run -p znimok-i18n -- stats [DIR]          # messages per language, complete or not
cargo run -p znimok-i18n -- sync-comments [DIR]  # header, order and comments from en.ftl
cargo run -p znimok-i18n -- po uk -o out/uk/LC_MESSAGES/znimok-app.po
cargo run -p znimok-i18n -- slint-refs           # the exact @tr(…) for every id
```

## Where the strings came from

The first version (28.09.2026, ZK-29) was written from the design mock-up v17 (25 artboards, both
the Ukrainian screens and the English artboard) and the feature list of Little Helpers' string
table — as a list of functions, not for compatibility. It covers v1 and the video screens of v2,
so the layout is stable from the first release. English wording is to be reviewed by Fable,
Ukrainian by the owner.
